//! Headless Native Shell UI tests for the Software Factory macOS app.

use nsh_core::prelude::{Size, px};
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

fn start() -> Headless {
    let mut config = EngineConfig::new(Root::of::<factory_app::App>());
    config.viewport = Size::new(px(1120.0), px(720.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("Software Factory").unwrap();
    app
}

#[test]
fn boots_with_starter_project_and_workflows() {
    let app = start();
    assert!(app.screen().shows("software-factory"));
    assert!(app.screen().shows("Feature Implementation"));
    assert!(app.screen().shows("Bug Triage & Fix"));
    assert!(!app.screen().enabled("role:button[Run Workflow]").unwrap());
}

#[test]
fn adds_project_and_runs_workflow_through_completion() {
    let mut app = start();

    // Register a new project via the sidebar form.
    app.type_text("text:Project name", "billing-api").unwrap();
    app.type_text("text:Repository path", "/workspace/billing-api")
        .unwrap();
    app.press("role:button[Add Project]").unwrap();
    app.wait_for("Added project 'billing-api'").unwrap();

    // Select the 3-step Bug Triage & Fix workflow and launch a run.
    app.press("Bug Triage & Fix").unwrap();
    app.type_text(
        "text:Describe work for the AI workflow to execute...",
        "Fix retry backoff jitter",
    )
    .unwrap();
    app.press("role:button[Run Workflow]").unwrap();
    app.wait_for("Started run #1").unwrap();
    app.wait_for("Run #1 · Queued (0/3)").unwrap();

    // Advance through all three steps to Succeeded.
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Running (1/3)").unwrap();
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Running (2/3)").unwrap();
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Succeeded (3/3)").unwrap();
}
