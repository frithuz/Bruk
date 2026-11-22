//! Headless Native Shell UI tests for Software Factory and SQLite project tracking.

use std::path::PathBuf;

use nsh_core::prelude::{Size, px};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use tempfile::TempDir;

fn start_with_db(db_path: PathBuf) -> Headless {
    let mut config = EngineConfig::new(factory_app::root_with_db_path(db_path));
    config.viewport = Size::new(px(1120.0), px(720.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("Software Factory").unwrap();
    app
}

#[test]
fn registers_project_in_sqlite_and_reloads_on_restart() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("software-factory").join("factory.db");

    {
        let mut app = start_with_db(db_path.clone());
        assert!(app.screen().shows("Register Project"));
        assert!(!app.screen().enabled("role:button[Save Project]").unwrap());

        app.type_text("text:Project name", "billing-api").unwrap();
        app.type_text("text:Repository path", "/workspace/billing-api")
            .unwrap();
        app.press("role:button[Save Project]").unwrap();
        app.wait_for("Registered project 'billing-api'").unwrap();
        assert!(app.screen().shows("/workspace/billing-api"));

        // Duplicate repository path should be rejected by SQLite UNIQUE constraint.
        app.type_text("text:Project name", "billing-dup").unwrap();
        app.type_text("text:Repository path", "/workspace/billing-api")
            .unwrap();
        app.press("role:button[Save Project]").unwrap();
        app.wait_for("a project is already registered for repository path '/workspace/billing-api'")
            .unwrap();
    }

    // Reopen the app against the same SQLite DB and verify persistence.
    let mut reopened = start_with_db(db_path);
    reopened.wait_for("billing-api").unwrap();
    assert!(reopened.screen().shows("/workspace/billing-api"));
    assert!(reopened.screen().shows("Feature Implementation"));
}

#[test]
fn opens_registered_project_and_runs_workflow_to_completion() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Register a project from the registration view, then open its workspace.
    app.type_text("text:Project name", "payments").unwrap();
    app.type_text("text:Repository path", "/workspace/payments")
        .unwrap();
    app.press("role:button[Save Project]").unwrap();
    app.wait_for("Registered project 'payments'").unwrap();

    app.press("role:button[Open Workspace]").unwrap();
    app.wait_for("Run Workflow").unwrap();

    // Select the 3-step Bug Triage & Fix workflow and run it through completion.
    app.press("Bug Triage & Fix").unwrap();
    app.type_text(
        "text:Describe work for the AI workflow to execute...",
        "Fix retry backoff jitter",
    )
    .unwrap();
    app.press("role:button[Run Workflow]").unwrap();
    app.wait_for("Started run #1").unwrap();
    app.wait_for("Run #1 · Queued (0/3)").unwrap();

    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Running (1/3)").unwrap();
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Running (2/3)").unwrap();
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Succeeded (3/3)").unwrap();
}
