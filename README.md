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

Run the native macOS app directly via Cargo:

```sh
cargo run
```

Run all unit tests and headless Native Shell UI tests:

```sh
cargo test --workspace
```

Run or package with the Native Shell CLI (once `nsh` is installed):

```sh
nsh run --target macos
nsh package --target macos
```
