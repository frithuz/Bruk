//! Headless Native Shell UI tests for the Dashboard landing page, Inbox, Machines,
//! SQLite project tracking, and workflow execution.

use std::path::PathBuf;

use nsh_core::prelude::{Size, px};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use tempfile::TempDir;

fn start_with_db(db_path: PathBuf) -> Headless {
    let mut config = EngineConfig::new(factory_app::root_with_db_path(db_path));
    config.viewport = Size::new(px(1360.0), px(920.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("Issues resolved").unwrap();
    app
}

#[test]
fn dashboard_landing_page_renders_reference_metrics_and_interactive_controls() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Header and top-level KPI cards.
    assert!(app.screen().shows("Dashboard"));
    assert!(
        app.screen()
            .shows("All projects · Mon Nov 9 – Sun Nov 15, 2026")
    );
    assert!(app.screen().shows("14"));
    assert!(app.screen().shows("4 more than the week before"));
    assert!(app.screen().shows("11"));
    assert!(app.screen().shows("52"));
    assert!(app.screen().shows("46 succeeded · 6 failed"));
    assert!(app.screen().shows("18.4M"));
    assert!(app.screen().shows("$63.20"));
    assert!(app.screen().shows("31.5 h"));

    // Main sections and sample rows.
    assert!(app.screen().shows("Tokens per day"));
    assert!(app.screen().shows("Tokens by project"));
    assert!(app.screen().shows("Projects worked on"));
    assert!(app.screen().shows("Highest token runs"));
    assert!(app.screen().shows("Dependency upgrade · run #6"));
    assert!(app.screen().shows("Time agents waited on you"));
    assert!(app.screen().shows("2 h 10 m"));
    assert!(app.screen().shows("Worth a look"));
    assert!(
        app.screen()
            .shows("Docs refresh failed 2 of its last 3 runs")
    );

    // Toggle Tokens per day between chart and table views.
    app.press("role:button[Show table]").unwrap();
    app.wait_for("Show chart").unwrap();
    assert!(app.screen().shows("Total"));
    app.press("role:button[Show chart]").unwrap();
    app.wait_for("Show table").unwrap();

    // Step backward a week and switch granularity to Month.
    app.press("role:button[Previous period]").unwrap();
    app.wait_for("2 weeks ago").unwrap();
    assert!(
        app.screen()
            .shows("All projects · Mon Nov 2 – Sun Nov 8, 2026")
    );

    app.press("role:button[Month]").unwrap();
    app.wait_for("November 2026").unwrap();
    assert!(
        app.screen()
            .shows("All projects · Sun Nov 1 – Mon Nov 30, 2026")
    );

    // Filter dashboard by clicking a project row in "Projects worked on".
    app.press("role:button[Week]").unwrap();
    app.wait_for("Last week").unwrap();
    app.press("role:button[Filter by api-gateway]").unwrap();
    app.wait_for("api-gateway · Mon Nov 9 – Sun Nov 15, 2026")
        .unwrap();
}

#[test]
fn inbox_and_machines_views_update_sidebar_badges() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Navigate to Inbox and approve a waiting request.
    app.press("Inbox").unwrap();
    app.wait_for("4 requests waiting for your input").unwrap();
    app.press("role:button[Approve] >> nth=0").unwrap();
    app.wait_for("3 requests waiting for your input").unwrap();

    // Navigate to Machines and reconnect the disconnected runner.
    app.press("Machines").unwrap();
    app.wait_for("2 connected · 1 disconnected").unwrap();
    app.press("role:button[Reconnect]").unwrap();
    app.wait_for("3 connected").unwrap();
}

#[test]
fn registers_project_in_sqlite_and_reloads_on_restart() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("software-factory").join("factory.db");

    {
        let mut app = start_with_db(db_path.clone());
        app.press("role:button[Register Project]").unwrap();
        app.wait_for("Register a local repository").unwrap();
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
        app.wait_for(
            "a project is already registered for repository path '/workspace/billing-api'",
        )
        .unwrap();
    }

    // Reopen the app against the same SQLite DB and verify persistence in the sidebar.
    let mut reopened = start_with_db(db_path);
    reopened.wait_for("billing-api").unwrap();
    reopened.press("billing-api >> nth=0").unwrap();
    reopened.wait_for("/workspace/billing-api").unwrap();
    assert!(reopened.screen().shows("Feature Implementation"));
}

#[test]
fn opens_registered_project_and_runs_workflow_to_completion() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Open project registration, save a project, then open its workspace.
    app.press("role:button[Register Project]").unwrap();
    app.wait_for("Register a local repository").unwrap();

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
