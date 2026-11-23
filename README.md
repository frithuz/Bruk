<img src="assets/icon.svg" width="64" height="64" alt="">

# Bruk

A native macOS desktop application built in **Rust** with **[Native Shell (`nsh`)](https://nsh.dev)** for orchestrating AI workflows across software projects.

## Architecture

This repository is a Cargo workspace split into two crates:

- **`crates/factory-core`** — Pure Rust domain model, the Bruk folder (`BrukHome`), SQLite project persistence (`ProjectDatabase`), and workflow execution engine (`Project`, `Workflow`, `WorkflowStep`, `WorkflowRun`, `StepExecutor`, and `FactoryState`), independent of UI/AppKit so workflows can be tested or reused from a CLI or daemon.
- **`crates/factory-app`** — Native Shell (`nsh-core`, `nsh-base`, `nsh-platform-macos`) macOS AppKit desktop application providing first-launch setup, a Project Registration view (clone into the Bruk folder or link an existing clone), project & workflow sidebar, task launcher, run list, and step/log inspector.
- **`nsh.json`** — Native Shell manifest pointing at `crates/factory-app` and its `App` root component.

## The Bruk Folder

Everything Bruk stores lives in one folder, `~/Bruk` by default:

```text
~/Bruk/
├── bruk.db           SQLite: projects, preferences, project settings, secrets, run history
├── projects/<name>/  Repositories Bruk cloned for you
├── runs/<run-id>/    Per-run step bundles and isolated git worktrees
└── cache/modules/    Git modules fetched for .agents/bruk/bruk.yaml
```

On first launch Bruk asks where to create this folder. Picking anything other than `~/Bruk`
is remembered in `~/Library/Application Support/Bruk/location` (one line, the folder's path).
Choosing a folder that already contains `bruk.db` reuses it as-is.

Projects come in two kinds:

- **Cloned** — Bruk runs `git clone` into the default projects directory, which is
  `projects/<name>` in the Bruk folder unless changed in Preferences › General (accepts any git
  URL, scp-style address, local path, or GitHub `owner/repo` shorthand).
- **External** — a clone you already have stays where it is; Bruk only records its path.

Removing a project from Bruk never deletes its files.

Data from the old location (`~/Library/Application Support/software-factory/factory.db`) is
offered for a one-time import during setup; the original file is left untouched.

Set `BRUK_HOME` to use a specific folder and skip setup (handy for development).

## Prerequisites

- macOS 14+ with Xcode Command Line Tools (`xcode-select --install`)
- Rust 1.97+ (`rustup`)
- *(Optional, for `.app` packaging and `nsh` CLI workflows)* Install the Native Shell CLI:
  ```sh
  curl -fsSL https://nsh.dev/install.sh | sh
  ```

## Development Commands

Common tasks are orchestrated with [Task](https://taskfile.dev/) via [Taskfile.yml](Taskfile.yml):

```sh
task                 # List all tasks
task run             # Run the native macOS app via Cargo (alias: task dev)
task test            # Run all unit + headless Native Shell UI tests
task test:core       # Run factory-core unit & SQLite tests
task test:ui         # Run factory-app headless UI tests
task fmt             # Format Rust code
task lint            # Run Clippy with -D warnings
task ci              # Run fmt:check, lint, test, and build
task db:path         # Show the active Bruk folder database path
task nsh:install     # Install/update the Native Shell CLI (v0.6.1)
task run:nsh         # Run with Native Shell CLI
task package         # Build .nsh/out/Bruk.app
task package:run     # Build and launch .nsh/out/Bruk.app
task package:universal # Build a universal (arm64 + x86_64) macOS .app bundle
```

Direct Cargo / Native Shell commands also work:

```sh
cargo run
cargo test --workspace
nsh run --target macos
nsh package --target macos
```
