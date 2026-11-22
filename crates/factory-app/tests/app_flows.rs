//! Headless Native Shell UI tests for the Dashboard landing page, Inbox, Machines,
//! SQLite project tracking, and workflow execution.

use std::path::PathBuf;

use nsh_core::{
    platform::Os,
    prelude::{Size, px},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use tempfile::TempDir;

fn start_with_db(db_path: PathBuf) -> Headless {
    let mut config = EngineConfig::new(factory_app::root_with_db_path(db_path));
    config.environment.os = Os::MacOs;
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

#[test]
fn preferences_window_manages_providers_tabs_and_persists_to_sqlite() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    {
        let mut app = start_with_db(db_path.clone());
        assert!(app.window_state("preferences").unwrap().is_none());

        // Open Preferences window from the sidebar entry.
        app.press("role:button[Preferences]").unwrap();
        app.wait_for("sk-or-••••••••••••81c0").unwrap();
        assert!(app.window_state("preferences").unwrap().is_some());

        // Verify reference Providers tab content matches 17 Preferences.png.
        assert!(app.screen().shows("Acme team"));
        assert!(app.screen().shows("OpenRouter · API key"));
        assert!(app.screen().shows("Key works"));
        assert!(app.screen().shows("$200.00 · $41.20 used"));
        assert!(app.screen().shows("openrouter/auto"));
        assert!(app.screen().shows("api-gateway · marketing-site"));

        // Promote Acme team to default OpenRouter key.
        app.press("role:button[Make Default]").unwrap();
        app.wait_for("Updated default OpenRouter key.").unwrap();

        // Update monthly spending cap inline.
        app.press("role:button[Edit...]").unwrap();
        let cap_selector = "id:prefs-inline-input";
        app.clear(cap_selector).unwrap();
        app.type_text(cap_selector, "350").unwrap();
        app.press("role:button[Save Cap]").unwrap();
        app.wait_for("$350.00 · $41.20 used").unwrap();

        // Add a new OpenRouter key via Add Account menu.
        app.press("role:button[Add Account]").unwrap();
        app.wait_for("Add OpenRouter key...").unwrap();
        app.press("role:button[Add OpenRouter key...]").unwrap();
        app.wait_for("Add OpenRouter API key").unwrap();
        app.type_text("id:prefs-new-key-name", "CI Runner").unwrap();
        app.type_text("id:prefs-new-key-secret", "sk-or-v1-testsecret99ef")
            .unwrap();
        app.press("role:button[Add Key]").unwrap();
        app.wait_for("sk-or-••••••••••••99ef").unwrap();
        assert!(app.screen().shows("CI Runner"));

        // Switch across other Preferences tabs and verify their controls.
        app.press("id:prefs-tab-appearance").unwrap();
        app.wait_for("Log & code font size").unwrap();
        app.press("role:button[Dark]").unwrap();

        app.press("id:prefs-tab-general").unwrap();
        app.wait_for("Default landing view").unwrap();

        app.press("id:prefs-tab-machines").unwrap();
        app.wait_for("Max concurrent runs per machine").unwrap();

        app.press("id:prefs-tab-extensions").unwrap();
        app.wait_for("Installed Extensions").unwrap();
        assert!(app.screen().shows("GitHub Issues & Pull Requests  v1.2.0"));

        app.press("id:prefs-tab-advanced").unwrap();
        app.wait_for("Provider CLI configs").unwrap();

        // Close the secondary Preferences window cleanly.
        app.press_window_close("preferences").unwrap();
        assert!(app.window_state("preferences").unwrap().is_none());
    }

    // Reopen app against the same SQLite database and verify preferences persisted.
    let mut reopened = start_with_db(db_path);
    reopened.press("role:button[Preferences]").unwrap();
    reopened.wait_for("CI Runner").unwrap();
    assert!(reopened.screen().shows("$350.00 · $41.20 used"));
}

#[test]
fn application_menu_shortcuts_navigate_views_toggle_sidebar_and_control_runs() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Window menu shortcuts switch top-level views.
    app.press_key("mod+i").unwrap();
    app.wait_for("4 requests waiting for your input").unwrap();

    app.press_key("alt+mod+m").unwrap();
    app.wait_for("2 connected · 1 disconnected").unwrap();

    app.press_key("mod+shift+d").unwrap();
    app.wait_for("Issues resolved").unwrap();

    // View › Hide Sidebar / Show Sidebar (⌃⌘S).
    assert!(app.screen().shows("Projects"));
    app.press_key("ctrl+cmd+s").unwrap();
    assert!(!app.screen().shows("Projects"));
    app.press_key("ctrl+cmd+s").unwrap();
    app.wait_for("Projects").unwrap();

    // App › Settings… (⌘,) and File › Close Window (⌘W) closes Preferences when focused.
    app.press_key("mod+,").unwrap();
    app.wait_for("sk-or-••••••••••••81c0").unwrap();
    assert!(app.window_state("preferences").unwrap().is_some());

    app.press_key("mod+w").unwrap();
    assert!(app.window_state("preferences").unwrap().is_none());

    // File › New Project… (⇧⌘N) opens project registration.
    app.press_key("mod+shift+n").unwrap();
    app.wait_for("Register a local repository").unwrap();

    // Run › Run History (⌘Y) opens the project workspace.
    app.press_key("mod+y").unwrap();
    app.wait_for("Workflow runs").unwrap();

    // Start the 4-step Feature Implementation workflow (ends with a Human Gate).
    app.type_text(
        "text:Describe work for the AI workflow to execute...",
        "Ship menubar commands",
    )
    .unwrap();
    app.press("role:button[Run Workflow]").unwrap();
    app.wait_for("Run #1 · Queued (0/4)").unwrap();

    // Advance 4 times to reach the WaitingForApproval gate.
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Running (1/4)").unwrap();
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Running (2/4)").unwrap();
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Running (3/4)").unwrap();
    app.press("role:button[Advance Step]").unwrap();
    app.wait_for("Run #1 · Awaiting Approval (3/4)").unwrap();

    // Run › Approve Step (⌘↩) approves the waiting human gate.
    app.press_key("mod+enter").unwrap();
    app.wait_for("Run #1 · Succeeded (4/4)").unwrap();

    // Start a second run and cancel it via Run › Stop (⇧⌘.).
    app.type_text(
        "text:Describe work for the AI workflow to execute...",
        "Cancelable run",
    )
    .unwrap();
    app.press("role:button[Run Workflow]").unwrap();
    app.wait_for("Run #2 · Queued (0/4)").unwrap();

    app.press_key("mod+shift+.").unwrap();
    app.wait_for("Cancelled run #2").unwrap();
    assert!(app.screen().shows("Run #2 · Cancelled (0/4)"));
}

#[test]
fn work_queue_renders_reference_board_inspector_and_interactive_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Open api-gateway's Work queue from the expanded sidebar tree.
    app.press("id:sidebar-work-queue-1").unwrap();
    app.wait_for("api-gateway · GitHub Issues · acme/api-gateway")
        .unwrap();

    // Verify reference banner, columns, cards, sidebar slot summary, and right inspector.
    assert!(app.screen().shows("Work queue"));
    assert!(app.screen().shows(
        "Open issues labeled agent-ready become work. Bugs go to Fix issue, docs to Docs refresh, everything else to Triage new issues."
    ));
    assert!(app.screen().shows("Ready for agents"));
    assert!(app.screen().shows("In progress"));
    assert!(app.screen().shows("Needs you"));
    assert!(app.screen().shows("Done this week"));
    assert!(app.screen().shows("Add request ID to error responses"));
    assert!(
        app.screen()
            .shows("Retry once on 503 from the auth service")
    );
    assert!(app.screen().shows("Document the rate-limit headers"));
    assert!(app.screen().shows("Move CI to Node 24"));
    assert!(app.screen().shows("Codex · loop attempt 1 of 3"));
    assert!(app.screen().shows("Expose rate-limit headers"));
    assert!(app.screen().shows("Claude · step 2 of 4"));
    assert!(app.screen().shows("Flaky session refresh test"));
    assert!(app.screen().shows("Review gate"));
    assert!(app.screen().shows("Return 429 instead of 500 on burst"));
    assert!(app.screen().shows("Typo in auth error message"));
    assert!(app.screen().shows("2 of 2 busy · 3 waiting"));
    assert!(app.screen().shows("acme/api-gateway #331"));
    assert!(app.screen().shows("Open on GitHub"));
    assert!(app.screen().shows("label bug → Fix issue"));
    assert!(app.screen().shows("1 of 3"));
    assert!(app.screen().shows("When an agent slot frees"));
    assert!(app.screen().shows("Software Factory will update the issue"));
    assert!(
        app.screen()
            .shows("9 open issues don't match the intake rules")
    );

    // Reassign workflow for #331 via the inspector picker.
    app.press("id:wq-inspector-use-another-workflow").unwrap();
    app.wait_for("Assign workflow").unwrap();
    app.press("role:button[Dependency upgrade]").unwrap();
    app.wait_for("manual override → Dependency upgrade")
        .unwrap();

    // Start #331 immediately from the inspector.
    app.press("id:wq-inspector-start-now").unwrap();
    app.wait_for("Started issue #331 as run #45").unwrap();
    assert!(app.screen().shows("3 of 2 busy · 2 waiting"));

    // Select #318 via Review button and approve its review gate.
    app.press("id:wq-card-review-318").unwrap();
    app.wait_for("acme/api-gateway #318").unwrap();
    app.press("id:wq-inspector-approve").unwrap();
    app.wait_for("Approved review gate for #318 — PR merged")
        .unwrap();

    // Toggle to List view and back to Board view.
    app.press("role:button[List]").unwrap();
    app.wait_for("id:wq-list-row-334").unwrap();
    app.press("role:button[Board]").unwrap();
    app.wait_for("id:wq-card-334").unwrap();

    // Expand unmatched issues and queue #338.
    app.press("id:wq-unmatched-toggle").unwrap();
    app.wait_for("Evaluate Envoy vs custom Rust ingress proxy")
        .unwrap();
    app.press("id:wq-queue-unmatched-338").unwrap();
    app.wait_for("8 open issues don't match the intake rules")
        .unwrap();

    // Sync pulls the next mocked issue (#340) from the backlog.
    app.press("id:wq-header-sync").unwrap();
    app.wait_for("Propagate traceparent header to downstream gRPC calls")
        .unwrap();

    // Launch an ad-hoc run from the header CTA.
    app.press("id:wq-header-adhoc").unwrap();
    app.wait_for("Start an ad-hoc run").unwrap();
    app.type_text("id:wq-adhoc-title", "Cut v2.4.0 release candidate")
        .unwrap();
    app.press("role:button[Start Ad-hoc Run]").unwrap();
    app.wait_for("Cut v2.4.0 release candidate").unwrap();

    // Edit and save intake rules inline.
    app.press("role:button[Edit intake rules]").unwrap();
    app.wait_for("Save Rules").unwrap();
    app.clear("id:wq-intake-required").unwrap();
    app.type_text("id:wq-intake-required", "bot-ready").unwrap();
    app.press("role:button[Save Rules]").unwrap();
    app.wait_for(
        "Open issues labeled bot-ready become work. Bugs go to Fix issue, docs to Docs refresh, everything else to Triage new issues.",
    )
    .unwrap();

    // Hide and restore the inspector via View › Hide Inspector (⌥⌘I).
    assert!(app.screen().shows("Software Factory will update the issue"));
    app.press_key("alt+mod+i").unwrap();
    assert!(!app.screen().shows("Software Factory will update the issue"));
    app.press_key("alt+mod+i").unwrap();
    app.wait_for("Software Factory will update the issue")
        .unwrap();
}

#[test]
fn workflow_run_page_renders_reference_release_prep_and_interactive_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Open api-gateway's Release prep workflow run from the sidebar tree.
    app.press("id:sidebar-workflow-1-release-prep").unwrap();
    app.wait_for("api-gateway · Run #42").unwrap();

    // Verify top header, sidebar extensions/providers, and footer status bar.
    assert!(app.screen().shows("Release prep"));
    assert!(app.screen().shows("Search or run a command"));
    assert!(app.screen().shows("Edit Workflow"));
    assert!(app.screen().shows("Deploy Watch"));
    assert!(app.screen().shows("2.7.3"));
    assert!(app.screen().shows("2.7.2"));
    assert!(app.screen().shows("Run history"));
    assert!(app.screen().shows("Providers"));
    assert!(
        app.screen()
            .shows("Codex running in worktree run-42 on release/2.8 · attempt 2 of 5")
    );
    assert!(app.screen().shows("Elapsed 4:02"));
    assert!(app.screen().shows("Est. cost $0.84"));
    assert!(app.screen().shows("4 extensions active"));

    // Verify run header and all 5 step pipeline cards.
    assert!(app.screen().shows("Run #42"));
    assert!(app.screen().shows("Running · step 3 of 5"));
    assert!(app.screen().shows("1. Gather changes"));
    assert!(app.screen().shows("2. Draft changelog"));
    assert!(app.screen().shows("3. Fix until tests pass"));
    assert!(app.screen().shows("Loop · attempt 2 of 5"));
    assert!(app.screen().shows("4. Review gate"));
    assert!(app.screen().shows("5. Open pull request"));

    // Verify Step 3 Output transcript and tool call cards.
    assert!(
        app.screen()
            .shows("Attempt 1 · 2 tests failed · output passed to attempt 2")
    );
    assert!(app.screen().shows("package.json"));
    assert!(app.screen().shows("npm version 2.8.0 --no-git-tag-version"));
    assert!(app.screen().shows("Exit 0"));
    assert!(app.screen().shows("npm test"));
    assert!(app.screen().shows("0:38"));
    assert!(app.screen().shows("src/router.test.ts"));
    assert!(app.screen().shows("src/ratelimit.test.ts"));

    // Verify right-hand Step 3 inspector sections.
    assert!(app.screen().shows("implementer · repository"));
    assert!(app.screen().shows("Codex CLI"));
    assert!(app.screen().shows("Pinned in agent"));
    assert!(app.screen().shows("Workspace write"));
    assert!(app.screen().shows("Isolated · Wald-managed"));
    assert!(app.screen().shows("Worktree"));
    assert!(app.screen().shows("release/2.8"));
    assert!(app.screen().shows("Attempt 2 of 5"));
    assert!(app.screen().shows("0 failing tests"));
    assert!(app.screen().shows("48.2k in · 6.1k out"));
    assert!(app.screen().shows("Step 2 output"));
    assert!(app.screen().shows("test-report · pending"));
    assert!(app.screen().shows("Budget Guard"));
    assert!(app.screen().shows("$0.84 / $5.00"));
    assert!(app.screen().shows("Next: approval gate"));

    // Switch between Output, Diff, and Coverage tabs.
    app.press("id:wf-tab-diff").unwrap();
    app.wait_for("3 files changed in worktree run-42").unwrap();
    assert!(app.screen().shows("CHANGELOG.md"));

    app.press("id:wf-tab-coverage").unwrap();
    app.wait_for("Test Coverage Report").unwrap();
    assert!(app.screen().shows("Stmts 94.2%"));

    // Jump back to Output via View › Output (⌘1).
    app.press_key("mod+1").unwrap();
    app.wait_for("npm test").unwrap();

    // Select Step 1 and verify inspector updates, then return to Step 3.
    app.press("id:wf-step-card-1").unwrap();
    app.wait_for("release-collector · repository").unwrap();
    app.press("id:wf-step-card-3").unwrap();
    app.wait_for("implementer · repository").unwrap();

    // Expand Attempt 1 details banner.
    app.press("id:wf-attempt-banner").unwrap();
    app.wait_for(
        "FAIL src/version.test.ts — expected APP_VERSION to equal \"2.7.3\", received \"2.8.0\" (2 assertions).",
    )
    .unwrap();

    // Send a steering instruction from the bottom composer.
    app.type_text("id:wf-run-steer-input", "skip the e2e suite")
        .unwrap();
    app.press("id:wf-run-steer-send").unwrap();
    app.wait_for("You steered this step").unwrap();
    assert!(app.screen().shows("skip the e2e suite"));

    // Open and close Command Palette from the header button.
    app.press("id:wf-header-search").unwrap();
    app.wait_for("Switch to Diff tab").unwrap();
    app.press("role:button[Close Command Palette]").unwrap();

    // Toggle inspector visibility with ⌥⌘I.
    app.press_key("alt+mod+i").unwrap();
    assert!(!app.screen().shows("Next: approval gate"));
    app.press_key("alt+mod+i").unwrap();
    app.wait_for("Next: approval gate").unwrap();

    // Pause, resume, and stop the active run via header controls.
    app.press("id:wf-header-pause").unwrap();
    app.wait_for("Paused · step 3 of 5").unwrap();
    app.press("id:wf-header-pause").unwrap();
    app.wait_for("Running · step 3 of 5").unwrap();
    app.press("id:wf-header-stop").unwrap();
    app.wait_for("Stopped · step 3 of 5").unwrap();
}
