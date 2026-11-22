<img src="assets/icon.svg" width="64" height="64" alt="">

# Software Factory

A native macOS desktop application built in **Rust** with **[Native Shell (`nsh`)](https://nsh.dev)** for orchestrating AI workflows across software projects.

## Architecture

This repository is a Cargo workspace split into two crates:

- **`crates/factory-core`** — Pure Rust domain model, SQLite project persistence (`ProjectDatabase` stored in the user's config folder), and workflow execution engine (`Project`, `Workflow`, `WorkflowStep`, `WorkflowRun`, `StepExecutor`, and `FactoryState`), independent of UI/AppKit so workflows can be tested or reused from a CLI or daemon.
- **`crates/factory-app`** — Native Shell (`nsh-core`, `nsh-base`, `nsh-platform-macos`) macOS AppKit desktop application providing a dedicated Project Registration view (persisted to SQLite with a native folder picker), project & workflow sidebar, task launcher, run list, and step/log inspector.
- **`nsh.json`** — Native Shell manifest pointing at `crates/factory-app` and its `App` root component.

## Local Project Database

Registered projects are persisted in SQLite at:

- macOS default: `~/Library/Application Support/software-factory/factory.db`
- XDG override: `$XDG_CONFIG_HOME/software-factory/factory.db`
- Explicit override: `SOFTWARE_FACTORY_DB_PATH`

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
task db:path         # Show the active SQLite database path
task run:nsh         # Run with Native Shell CLI (requires nsh)
task package         # Package a macOS .app bundle (requires nsh)
```

Direct Cargo / Native Shell commands also work:

```sh
cargo run
cargo test --workspace
nsh run --target macos
nsh package --target macos
```
