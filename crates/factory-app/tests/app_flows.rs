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
    app.wait_for("Dashboard").unwrap();
    app
}

fn enable_demo_mode_in_db(db_path: &std::path::Path) {
    let db = factory_core::ProjectDatabase::open(db_path).unwrap();
    let mut prefs = db
        .load_preferences()
        .unwrap()
        .unwrap_or_else(factory_core::AppPreferences::empty);
    prefs.demo_mode = true;
    db.save_preferences(&prefs).unwrap();
}

fn start_with_demo_db(db_path: PathBuf) -> Headless {
    enable_demo_mode_in_db(&db_path);
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

    let mut app = start_with_demo_db(db_path);

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

    let mut app = start_with_demo_db(db_path);

    // Navigate to Inbox inside the primary window and answer a waiting request.
    app.press("id:sidebar-inbox-btn").unwrap();
    app.wait_for("5 agents waiting · oldest 25 min · 3 still working")
        .unwrap();
    assert!(app.window_state("inbox").unwrap().is_none());
    app.press("id:inbox-allow-once").unwrap();
    app.wait_for("4 agents waiting · oldest 25 min · 3 still working")
        .unwrap();

    // Navigate to Machines and verify the fleet header and reconnect flow.
    app.press("Machines").unwrap();
    app.wait_for("5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Bruk")
        .unwrap();
    app.press("id:machine-card-win-ci-02").unwrap();
    app.wait_for("Windows 11 24H2").unwrap();
    app.press("id:machine-action-connection").unwrap();
    app.wait_for("Caught up just now").unwrap();
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
        let mut app = start_with_demo_db(db_path.clone());
        assert!(app.window_state("preferences").unwrap().is_none());

        // Sidebar does not expose a Preferences button; open via the app menu shortcut (⌘,).
        assert!(!app.screen().matches("role:button[Preferences]").unwrap());
        app.press_key("mod+,").unwrap();
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
        app.wait_for("Ubuntu 24.04.1 LTS").unwrap();
        assert!(app.screen().shows("Test Connection"));

        app.press("id:prefs-tab-extensions").unwrap();
        app.wait_for("Installed Extensions").unwrap();
        assert!(app.screen().shows("GitHub Issues & Pull Requests  v1.2.0"));

        app.press("id:prefs-tab-advanced").unwrap();
        app.wait_for("Provider CLI configs").unwrap();

        // Close the secondary Preferences window cleanly.
        app.press_window_close("preferences").unwrap();
        assert!(app.window_state("preferences").unwrap().is_none());
    }

    // Reopen app against a real (non-demo) SQLite database and verify real preferences persist
    // cleanly without any Demo Mode fixtures leaking in.
    let real_temp = TempDir::new().unwrap();
    let real_db_path = real_temp.path().join("factory.db");
    {
        let mut real_app = start_with_db(real_db_path.clone());
        real_app.press_key("mod+,").unwrap();
        real_app.wait_for("No OpenRouter keys configured").unwrap();
        assert!(!real_app.screen().shows("Acme team"));

        real_app.press("role:button[Add Account]").unwrap();
        real_app.wait_for("Add OpenRouter key...").unwrap();
        real_app
            .press("role:button[Add OpenRouter key...]")
            .unwrap();
        real_app.wait_for("Add OpenRouter API key").unwrap();
        real_app
            .type_text("id:prefs-new-key-name", "CI Runner")
            .unwrap();
        real_app
            .type_text("id:prefs-new-key-secret", "sk-or-v1-testsecret99ef")
            .unwrap();
        real_app.press("role:button[Add Key]").unwrap();
        real_app.wait_for("sk-or-••••••••••••99ef").unwrap();

        real_app.press("role:button[Edit...]").unwrap();
        let cap_selector = "id:prefs-inline-input";
        real_app.clear(cap_selector).unwrap();
        real_app.type_text(cap_selector, "350").unwrap();
        real_app.press("role:button[Save Cap]").unwrap();
        real_app.wait_for("$350.00 · $0.00 used").unwrap();
        real_app.press_window_close("preferences").unwrap();
    }

    let mut reopened = start_with_db(real_db_path);
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("CI Runner").unwrap();
    assert!(reopened.screen().shows("$350.00 · $0.00 used"));
    assert!(!reopened.screen().shows("Acme team"));
}

#[test]
fn application_menu_shortcuts_navigate_views_toggle_sidebar_and_control_runs() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);

    // Window menu shortcuts switch top-level views inside the primary window.
    app.press_key("mod+i").unwrap();
    app.wait_for("5 agents waiting · oldest 25 min · 3 still working")
        .unwrap();
    assert!(app.window_state("inbox").unwrap().is_none());

    app.press_key("alt+mod+m").unwrap();
    app.wait_for("5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Bruk")
        .unwrap();

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

    let mut app = start_with_demo_db(db_path);

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
    assert!(app.screen().shows("Bruk will update the issue"));
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
    assert!(app.screen().shows("Bruk will update the issue"));
    app.press_key("alt+mod+i").unwrap();
    assert!(!app.screen().shows("Bruk will update the issue"));
    app.press_key("alt+mod+i").unwrap();
    app.wait_for("Bruk will update the issue").unwrap();
}

#[test]
fn workflow_run_page_renders_reference_release_prep_and_interactive_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);

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
    assert!(app.screen().shows("Isolated · Bruk-managed"));
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

#[test]
fn project_settings_view_renders_issues_intake_and_interactive_tabs() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);
    assert!(app.window_state("project-settings").unwrap().is_none());

    // Open api-gateway Project Settings inside the main panel from the expanded sidebar tree.
    app.press("id:sidebar-project-settings-1").unwrap();
    app.wait_for("api-gateway · Issues & intake").unwrap();
    assert!(app.window_state("project-settings").unwrap().is_none());
    assert!(app.screen().shows("Project settings"));

    // Verify all 6 left sidebar tabs.
    assert!(app.screen().shows("General"));
    assert!(app.screen().shows("Issues & intake"));
    assert!(app.screen().shows("Providers"));
    assert!(app.screen().shows("Workflows"));
    assert!(app.screen().shows("Environment"));
    assert!(app.screen().shows("Agents & limits"));

    // Verify Issues & intake content matches 05 ProjectSettings.png.
    assert!(app.screen().shows("Issue source"));
    assert!(
        app.screen()
            .shows("Where this project's work comes from. Each project uses one source.")
    );
    assert!(app.screen().shows("GitHub Issues"));
    assert!(app.screen().shows("Issues and labels from one repository"));
    assert!(app.screen().shows("itsaplan"));
    assert!(
        app.screen()
            .shows("Work items delegated to an agent member")
    );
    assert!(app.screen().shows("From an extension"));
    assert!(app.screen().shows("No issue source extensions installed"));
    assert_eq!(
        app.screen().input_text("id:ps-github-repo").unwrap(),
        Some("acme/api-gateway")
    );
    assert_eq!(
        app.screen().input_text("id:ps-concurrency-input").unwrap(),
        Some("2")
    );
    assert!(app.screen().shows("Your GitHub CLI (gh) sign-in"));
    assert!(app.screen().shows("Use a Token Instead..."));
    assert!(
        app.screen()
            .shows("Connected · 14 open issues · synced 1 min ago")
    );

    assert!(app.screen().shows("Intake rules"));
    assert!(app.screen().shows(
        "Open issues labeled agent-ready become work. The first matching rule picks the workflow."
    ));
    assert!(app.screen().shows("Order"));
    assert!(app.screen().shows("When the issue"));
    assert!(app.screen().shows("Run workflow"));
    assert!(app.screen().shows("has label bug"));
    assert!(app.screen().shows("Fix issue"));
    assert!(app.screen().shows("has label docs"));
    assert!(app.screen().shows("Docs refresh"));
    assert!(app.screen().shows("matches anything else"));
    assert!(app.screen().shows("Triage new issues"));
    assert!(app.screen().shows("Add Rule"));
    assert!(app.screen().shows("Runs at once for this project"));

    assert!(app.screen().shows("Start matching work automatically"));
    assert!(
        app.screen()
            .shows("Off: issues wait in Ready for agents until you press Start.")
    );
    assert!(app.screen().shows("Update the issue as work moves"));
    assert!(
        app.screen()
            .shows("Add the label agent-working while a run is active")
    );
    assert!(
        app.screen()
            .shows("Comment with the run link and the final summary")
    );
    assert!(
        app.screen()
            .shows("Link the pull request so merging closes the issue")
    );

    // Switch GitHub sign-in to a personal access token and back to GitHub CLI.
    app.press("id:ps-use-token-btn").unwrap();
    app.wait_for("id:ps-token-input").unwrap();
    app.type_text("id:ps-token-input", "ghp_placeholder_secret_9a2c")
        .unwrap();
    app.press("id:ps-token-save").unwrap();
    app.wait_for("Personal access token (ghp_••••••••••••9a2c)")
        .unwrap();
    app.press("id:ps-use-gh-cli-btn").unwrap();
    app.wait_for("Your GitHub CLI (gh) sign-in").unwrap();

    // Add a new intake rule and verify it appears before the fallback row.
    app.press("id:ps-add-rule-btn").unwrap();
    app.wait_for("id:ps-new-rule-label").unwrap();
    app.type_text("id:ps-new-rule-label", "security").unwrap();
    app.press("id:ps-rule-save").unwrap();
    app.wait_for("has label security").unwrap();

    // Switch issue source to itsaplan and back to GitHub Issues.
    app.press("id:ps-source-itsaplan").unwrap();
    app.wait_for("Signed in as @factory-bot via workspace token")
        .unwrap();
    app.press("id:ps-source-github").unwrap();
    app.wait_for("Connected · 14 open issues · synced 1 min ago")
        .unwrap();

    // Toggle auto-start and issue-update checkboxes.
    app.press("id:ps-auto-start-checkbox").unwrap();
    app.press("id:ps-update-working-label").unwrap();
    app.press("id:ps-update-comment-summary").unwrap();
    app.press("id:ps-update-link-pr").unwrap();

    // Navigate through all remaining sidebar tabs and verify placeholder content.
    app.press("id:ps-tab-general").unwrap();
    app.wait_for("Branches & worktrees").unwrap();
    assert!(
        app.screen()
            .shows("origin · git@github.com:acme/api-gateway.git")
    );

    app.press("id:ps-tab-providers").unwrap();
    app.wait_for("Routing & credentials").unwrap();
    assert!(app.screen().shows("Open Provider Preferences..."));

    app.press("id:ps-tab-workflows").unwrap();
    app.wait_for("Discovered workflows").unwrap();
    assert!(
        app.screen()
            .shows(".factory/workflows/fix-issue.yaml · Intake · 4 steps · Claude / Codex")
    );

    app.press("id:ps-tab-environment").unwrap();
    app.wait_for("STAGING_API_TOKEN").unwrap();
    assert!(app.screen().shows("••••••••••••9b4e"));
    app.press("id:ps-env-add-btn").unwrap();
    app.wait_for("id:ps-env-key-input").unwrap();
    app.type_text("id:ps-env-key-input", "API_TIMEOUT_MS")
        .unwrap();
    app.type_text("id:ps-env-value-input", "30000").unwrap();
    app.press("id:ps-env-save").unwrap();
    app.wait_for("API_TIMEOUT_MS").unwrap();

    app.press("id:ps-tab-agents-limits").unwrap();
    app.wait_for("Budget & guardrails").unwrap();
    assert!(app.screen().shows("api-gateway · Agents & limits"));
    assert!(app.screen().shows("$5.00"));
    assert!(app.screen().shows("$120.00"));
    assert!(app.window_state("project-settings").unwrap().is_none());

    // Navigate back to Dashboard inside the main window cleanly.
    app.press_key("mod+shift+d").unwrap();
    app.wait_for("Issues resolved").unwrap();
    assert!(app.window_state("project-settings").unwrap().is_none());
}

#[test]
fn workflows_overview_renders_reference_table_inspector_templates_and_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);

    // Open api-gateway's Workflows overview from the expanded sidebar tree.
    app.press("id:sidebar-workflows-1").unwrap();
    app.wait_for("api-gateway · 6 workflows · 2 running")
        .unwrap();

    // Verify header, filter tabs, sort control, table columns, and all 6 reference workflows.
    assert!(app.screen().shows("Workflows"));
    assert!(app.screen().shows("Import..."));
    assert!(app.screen().shows("New Workflow"));
    assert!(app.screen().shows("All"));
    assert!(app.screen().shows("Active"));
    assert!(app.screen().shows("Picks up issues"));
    assert!(app.screen().shows("Drafts"));
    assert!(app.screen().shows("Sort by"));
    assert!(app.screen().shows("Last run"));
    assert!(app.screen().shows("Starts from"));
    assert!(app.screen().shows("Last 10 runs"));
    assert!(app.screen().shows("Now"));

    assert!(app.screen().shows("Fix issue"));
    assert!(
        app.screen()
            .shows("Plans, fixes until tests pass, then opens a PR")
    );
    assert!(app.screen().shows("Work queue · bug"));
    assert!(app.screen().shows("Last run 3 min ago"));
    assert!(app.screen().shows("2 active · 1 needs you"));

    assert!(app.screen().shows("Triage new issues"));
    assert!(
        app.screen()
            .shows("Labels, asks for repro steps, sizes the work")
    );
    assert!(app.screen().shows("Work queue · other"));
    assert!(app.screen().shows("Last run 6 min ago"));

    assert!(app.screen().shows("Dependency upgrade"));
    assert!(
        app.screen()
            .shows("Upgrades one dependency and fixes fallout")
    );
    assert!(app.screen().shows("Work queue · chore"));
    assert!(app.screen().shows("Last run 22 min ago"));

    assert!(app.screen().shows("Docs refresh"));
    assert!(
        app.screen()
            .shows("Updates docs pages affected by recent changes")
    );
    assert!(app.screen().shows("Work queue · docs"));
    assert!(app.screen().shows("Failed yesterday"));
    assert!(app.screen().shows("Idle"));

    assert!(app.screen().shows("Release prep"));
    assert!(app.screen().shows("Changelog, version bump, release PR"));
    assert!(app.screen().shows("Ad-hoc only"));
    assert!(app.screen().shows("Last run 4 min ago"));

    assert!(app.screen().shows("Security audit"));
    assert!(
        app.screen()
            .shows("Reviews auth code paths and reports findings")
    );
    assert!(app.screen().shows("Never run"));
    assert!(app.screen().shows("Draft"));

    // Verify run outcome legend and all 4 starter template cards.
    assert!(app.screen().shows("Succeeded"));
    assert!(app.screen().shows("Failed or stopped"));
    assert!(app.screen().shows("Running or waiting"));
    assert!(app.screen().shows("No run yet"));
    assert!(app.screen().shows("Start from a template"));
    assert!(app.screen().shows("Blank workflow"));
    assert!(app.screen().shows("Start with one step"));
    assert!(app.screen().shows("Fix an issue"));
    assert!(app.screen().shows("Plan, fix in a loop, review, PR"));
    assert!(app.screen().shows("Review a pull request"));
    assert!(app.screen().shows("Two models compare notes"));
    assert!(app.screen().shows("Import from file..."));
    assert!(app.screen().shows("A .bruk-workflow file"));

    // Verify default right-hand inspector for Fix issue.
    assert!(app.screen().shows("Edit"));
    assert!(app.screen().shows("Ad-hoc Run..."));
    assert!(app.screen().shows("Steps"));
    assert!(app.screen().shows("Read issue & plan"));
    assert!(app.screen().shows("Fix until tests pass · max 5"));
    assert!(app.screen().shows("until pass"));
    assert!(app.screen().shows("Implement fix"));
    assert!(app.screen().shows("Run tests"));
    assert!(app.screen().shows("Review gate"));
    assert!(app.screen().shows("Open pull request"));
    assert!(app.screen().shows("Comment on issue"));
    assert!(app.screen().shows("Issues labeled bug"));
    assert!(app.screen().shows("Allowed"));
    assert!(app.screen().shows("14 min"));
    assert!(app.screen().shows("$1.12"));
    assert!(app.screen().shows("Today by you"));
    assert!(app.screen().shows("Recent runs"));
    assert!(app.screen().shows("Issue #325 · Move CI to Node 24"));
    assert!(app.screen().shows("Issue #318 · Flaky session test"));
    assert!(app.screen().shows("Issue #310 · 429 on burst"));
    assert!(app.screen().shows("Issue #301 · Header casing"));

    // Selecting Security audit updates the right-hand inspector.
    app.press("id:wfo-row-security-audit").unwrap();
    app.wait_for("Map auth & token paths").unwrap();
    assert!(app.screen().shows("No runs yet"));

    // Filter with Drafts and search input, then return to All.
    app.press("id:wfo-filter-drafts").unwrap();
    assert!(!app.screen().shows("Last run 3 min ago"));
    assert!(app.screen().shows("Never run"));

    app.press("id:wfo-filter-all").unwrap();
    app.wait_for("Last run 3 min ago").unwrap();

    app.type_text("id:wfo-header-filter", "changelog").unwrap();
    assert!(app.screen().shows("Changelog, version bump, release PR"));
    assert!(!app.screen().shows("Last run 3 min ago"));
    app.clear("id:wfo-header-filter").unwrap();
    app.wait_for("Last run 3 min ago").unwrap();

    // Launch an ad-hoc run for Fix issue from the inspector.
    app.press("id:wfo-row-fix-issue").unwrap();
    app.wait_for("Fix until tests pass · max 5").unwrap();
    app.press("id:wfo-inspector-adhoc").unwrap();
    app.wait_for("id:wfo-adhoc-input").unwrap();
    app.type_text("id:wfo-adhoc-input", "Issue #345 · Audit timeout")
        .unwrap();
    app.press("id:wfo-adhoc-start").unwrap();
    app.wait_for("Issue #345 · Audit timeout").unwrap();
    assert!(app.screen().shows("api-gateway · 6 workflows · 3 running"));

    // Create a draft workflow from the Review a pull request template card.
    app.press("id:wfo-template-review-pr").unwrap();
    app.wait_for("Primary diff review").unwrap();
    assert!(app.screen().shows("api-gateway · 7 workflows · 3 running"));

    // Create a custom workflow via the header + New Workflow drawer.
    app.press("id:wfo-header-new").unwrap();
    app.wait_for("Create a new workflow").unwrap();
    app.type_text("id:wfo-new-name", "Nightly smoke tests")
        .unwrap();
    app.type_text("id:wfo-new-desc", "Runs staging smoke suite every night")
        .unwrap();
    app.press("id:wfo-new-submit").unwrap();
    app.wait_for("Nightly smoke tests").unwrap();
    assert!(app.screen().shows("api-gateway · 8 workflows · 3 running"));

    // Import a .bruk-workflow file via the Import... drawer.
    app.press("id:wfo-header-import").unwrap();
    app.wait_for("Import workflow from file").unwrap();
    app.press("id:wfo-import-submit").unwrap();
    app.wait_for("Canary deploy").unwrap();
    assert!(app.screen().shows("api-gateway · 9 workflows · 3 running"));

    // Hide and restore the inspector via View › Hide Inspector (⌥⌘I).
    assert!(app.screen().shows("Recent runs"));
    app.press_key("alt+mod+i").unwrap();
    assert!(!app.screen().shows("Recent runs"));
    app.press_key("alt+mod+i").unwrap();
    app.wait_for("Recent runs").unwrap();

    // Clicking a recent run on Fix issue navigates to the live WorkflowRun view.
    app.press("id:wfo-row-fix-issue").unwrap();
    app.wait_for("id:wfo-recent-run-44").unwrap();
    app.press("id:wfo-recent-run-44").unwrap();
    app.wait_for("Edit Workflow").unwrap();
}

#[test]
fn workflow_editor_renders_reference_layout_and_interactive_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);

    // Navigate to api-gateway's Workflows overview and open Fix issue in the visual editor.
    app.press("id:sidebar-workflows-1").unwrap();
    app.wait_for("id:wfo-inspector-edit").unwrap();
    app.press("id:wfo-inspector-edit").unwrap();
    app.wait_for("Unsaved changes").unwrap();

    // Verify top header breadcrumb and action buttons.
    assert!(app.screen().shows("api-gateway"));
    assert!(app.screen().shows("Workflows"));
    assert!(app.screen().shows("Fix issue"));
    assert!(app.screen().shows("Unsaved changes"));
    assert!(app.screen().shows("Validate"));
    assert!(app.screen().shows("Save"));
    assert!(app.screen().shows("Ad-hoc Run..."));

    // Verify left step types palette categories and entries.
    assert!(app.screen().shows("Agents"));
    assert!(app.screen().shows("Claude task"));
    assert!(app.screen().shows("Claude Code in the project"));
    assert!(app.screen().shows("Codex task"));
    assert!(app.screen().shows("Codex CLI in the project"));
    assert!(app.screen().shows("Model call"));
    assert!(app.screen().shows("One OpenRouter request"));
    assert!(app.screen().shows("Control"));
    assert!(app.screen().shows("Loop"));
    assert!(app.screen().shows("Repeat steps until a check passes"));
    assert!(app.screen().shows("Condition"));
    assert!(app.screen().shows("Approval gate"));
    assert!(app.screen().shows("Shell command"));
    assert!(app.screen().shows("Composites"));
    assert!(app.screen().shows("Review and open PR"));
    assert!(app.screen().shows("Two-model review"));
    assert!(app.screen().shows("Issues"));
    assert!(app.screen().shows("Set issue status"));
    assert!(app.screen().shows("From extensions"));
    assert!(app.screen().shows("Post update"));
    assert!(app.screen().shows("Chat Notifier"));

    // Verify center visual workflow canvas matches 13 WorkflowEditor.png.
    assert!(app.screen().shows("Starts from"));
    assert!(app.screen().shows("Work queue · intake rules"));
    assert!(app.screen().shows("Ad-hoc runs allowed"));
    assert!(
        app.screen()
            .shows("inputs: issue (optional for ad-hoc) · branch")
    );
    assert!(app.screen().shows("Read issue & plan"));
    assert!(app.screen().shows("Fix until tests pass"));
    assert!(app.screen().shows("Repeat until · max 5"));
    assert!(app.screen().shows("Implement fix"));
    assert!(app.screen().shows("reads $loop.previous on attempt 2+"));
    assert!(app.screen().shows("Run tests"));
    assert!(app.screen().shows("Shell · npm test"));
    assert!(app.screen().shows("steps.run_tests.exit_code = 0"));
    assert!(app.screen().shows("Out of attempts"));
    assert!(app.screen().shows("If out of attempts"));
    assert!(app.screen().shows("Report failure"));
    assert!(app.screen().shows("Comment on issue · stop run"));
    assert!(app.screen().shows("Review gate"));
    assert!(app.screen().shows("Open pull request"));
    assert!(app.screen().shows("Comment on issue"));
    assert!(app.screen().shows("Add step"));

    // Verify right-hand Loop inspector controls for the default selected Loop.
    assert!(app.screen().shows("Loop · wraps steps 2–3"));
    assert!(app.screen().shows("Repeat"));
    assert!(app.screen().shows("Until a check passes"));
    assert!(app.screen().shows("retry loop"));
    assert!(app.screen().shows("For each item in a list"));
    assert!(app.screen().shows("e.g. each failing test"));
    assert!(app.screen().shows("A fixed number of times"));
    assert!(app.screen().shows("Exit when"));
    assert!(app.screen().shows("3 · Run tests"));
    assert!(app.screen().shows("exit code"));
    assert!(app.screen().shows("+ Add condition (all must pass)"));
    assert!(app.screen().shows("Max attempts"));
    assert!(app.screen().shows("Wait between"));
    assert!(app.screen().shows("None"));
    assert!(app.screen().shows("Between attempts"));
    assert!(
        app.screen()
            .shows("Pass the last attempt's outputs to the next one as")
    );
    assert!(app.screen().shows("$loop.previous"));
    assert!(
        app.screen()
            .shows("Discard file changes before each attempt")
    );
    assert!(app.screen().shows("If attempts run out"));
    assert!(app.screen().shows("Go to step: Report failure"));

    // Filter the step palette and clear it back.
    app.type_text("id:wfe-palette-search", "Chat Notifier")
        .unwrap();
    assert!(app.screen().shows("Post update"));
    assert!(!app.screen().shows("Claude Code in the project"));
    app.clear("id:wfe-palette-search").unwrap();
    app.wait_for("Claude Code in the project").unwrap();

    // Update loop settings in the inspector and verify canvas updates live.
    app.press("id:wfe-repeat-fixed").unwrap();
    app.wait_for("Repeat · 5 times").unwrap();
    app.press("id:wfe-repeat-until").unwrap();
    app.wait_for("Repeat until · max 5").unwrap();

    app.press("id:wfe-exit-add-condition").unwrap();
    app.wait_for("steps.run_tests.exit_code = 0 && steps.run_tests.status = ok")
        .unwrap();

    app.press("id:wfe-loop-pass-prev").unwrap();
    assert!(!app.screen().shows("reads $loop.previous on attempt 2+"));
    app.press("id:wfe-loop-pass-prev").unwrap();
    app.wait_for("reads $loop.previous on attempt 2+").unwrap();

    // Validate the workflow and dismiss the validation banner.
    app.press("id:wfe-header-validate").unwrap();
    app.wait_for("Valid workflow — 6 steps, 1 loop, all exit conditions resolved.")
        .unwrap();
    app.press("id:wfe-validation-dismiss").unwrap();

    // Save clears the Unsaved changes badge.
    app.press("id:wfe-header-save").unwrap();
    assert!(!app.screen().shows("Unsaved changes"));

    // Adding a step from the palette marks unsaved changes again and switches the inspector.
    app.press("id:wfe-palette-post-update").unwrap();
    app.wait_for("Unsaved changes").unwrap();
    assert!(app.screen().shows("Step 7 · Post update"));

    // Select the fallback card to inspect it, then return to Workflows via the back button.
    app.press("id:wfe-node-fallback").unwrap();
    app.wait_for("Fallback · If out of attempts").unwrap();

    // Toggle inspector visibility via View › Hide Inspector (⌥⌘I).
    app.press_key("alt+mod+i").unwrap();
    assert!(!app.screen().shows("Fallback · If out of attempts"));
    app.press_key("alt+mod+i").unwrap();
    app.wait_for("Fallback · If out of attempts").unwrap();

    // Navigate back to Workflows overview.
    app.press("id:wfe-header-back").unwrap();
    app.wait_for("api-gateway · 6 workflows · 2 running")
        .unwrap();

    // Also verify opening the editor from WorkflowRun's `Edit Workflow` header button.
    app.press("id:wfo-row-fix-issue").unwrap();
    app.wait_for("id:wfo-recent-run-44").unwrap();
    app.press("id:wfo-recent-run-44").unwrap();
    app.wait_for("id:wf-header-edit").unwrap();
    app.press("id:wf-header-edit").unwrap();
    app.wait_for("Starts from").unwrap();
    assert!(app.screen().shows("Fix issue"));
}

#[test]
fn machines_page_renders_reference_layout_and_interactive_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);

    // Navigate to Machines from the sidebar.
    assert!(
        app.screen()
            .matches("role:button[Machines, 5 registered]")
            .unwrap()
    );
    app.press("Machines").unwrap();
    app.wait_for("5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Bruk")
        .unwrap();

    // Verify top header buttons.
    assert!(app.screen().shows("Update Cores..."));
    assert!(app.screen().shows("Add Machine..."));

    // Verify all 5 machines and compatibility pills in the left list pane.
    assert!(app.screen().shows("This Mac"));
    assert!(app.screen().shows("macOS 26.1 · arm64"));
    assert!(app.screen().shows("3 sessions"));
    assert!(app.screen().shows("build-box-01"));
    assert!(app.screen().shows("Ubuntu 24.04 · x86_64"));
    assert!(app.screen().shows("Caught up 2 min ago"));
    assert!(app.screen().shows("4 sessions"));
    assert!(app.screen().shows("win-ci-02"));
    assert!(app.screen().shows("Windows 11 24H2 · x86_64"));
    assert!(app.screen().shows("Last seen 3 h ago"));
    assert!(app.screen().shows("Core update suggested"));
    assert!(app.screen().shows("gpu-01"));
    assert!(app.screen().shows("Fedora 41 · x86_64"));
    assert!(app.screen().shows("Not syncing"));
    assert!(app.screen().shows("Needs a newer Bruk"));
    assert!(app.screen().shows("mini-lab"));
    assert!(app.screen().shows("Unreachable"));
    assert!(app.screen().shows("Since Sunday"));
    assert!(
        app.screen()
            .shows("Machines connect over SSH with a key. Sessions keep running on a machine when Bruk disconnects, and catch up when it reconnects. On every connection Bruk and the core compare data schemas; version numbers only matter when the schema differs.")
    );

    // Verify default selected build-box-01 detail pane matches 21 Machines.png.
    assert!(app.screen().shows("Open in Terminal"));
    assert!(app.screen().shows("Disconnect"));
    assert!(
        app.screen()
            .shows("Reconnected 2 minutes ago and caught up")
    );
    assert!(
        app.screen()
            .shows("While Bruk was away, the 4 sessions here produced 318 events: 3 steps finished, one loop ran out of attempts, and 2 new requests are in the Inbox.")
    );
    assert!(app.screen().shows("Show What Changed"));
    assert!(app.screen().shows("Ubuntu 24.04.1 LTS"));
    assert!(app.screen().shows("x86_64 · 16 cores · 64 GB"));
    assert!(app.screen().shows("10.0.4.21:22"));
    assert!(app.screen().shows("fd00::4:21"));
    assert!(app.screen().shows("bruk@build-box-01"));
    assert!(app.screen().shows("ED25519 · SHA256:q3Ff…8k…"));
    assert!(app.screen().shows("0.8.5 · schema 7"));
    assert!(app.screen().shows("Same schema · in sync"));
    assert!(app.screen().shows("Live again after catching up"));
    assert!(app.screen().shows("#44"));
    assert!(app.screen().shows("api-gateway › Dependency upgrade"));
    assert!(app.screen().shows("Needs you · permission"));
    assert!(app.screen().shows("#12"));
    assert!(app.screen().shows("marketing-site › Docs refresh"));
    assert!(app.screen().shows("Needs you · question"));
    assert!(app.screen().shows("#7"));
    assert!(app.screen().shows("mobile-app › Dependency upgrade"));
    assert!(app.screen().shows("Out of attempts"));
    assert!(app.screen().shows("#45"));
    assert!(app.screen().shows("api-gateway › Fix issue"));
    assert!(app.screen().shows("Step 1 of 6"));
    assert!(app.screen().shows("Machine settings..."));
    assert!(
        app.screen()
            .shows("Projects that can run here: api-gateway, marketing-site, mobile-app")
    );

    // Expand and collapse "Show What Changed" in the catch-up banner.
    app.press("id:machine-banner-show-changes").unwrap();
    app.wait_for("Hide Changes").unwrap();
    assert!(app.screen().shows(
        "#44 api-gateway › Dependency upgrade — paused for permission in Inbox (214 events)"
    ));
    app.press("id:machine-banner-show-changes").unwrap();
    app.wait_for("Show What Changed").unwrap();

    // Open Machine settings and toggle project access on build-box-01.
    app.press("id:machine-settings-toggle").unwrap();
    app.wait_for("Allowed projects").unwrap();
    app.press("id:machine-setting-project-mobile-app").unwrap();
    app.wait_for("Projects that can run here: api-gateway, marketing-site")
        .unwrap();
    app.press("id:machine-setting-project-mobile-app").unwrap();
    app.wait_for("Projects that can run here: api-gateway, marketing-site, mobile-app")
        .unwrap();

    // Switch to gpu-01 and mini-lab to verify contextual banners and empty session table.
    app.press("id:machine-card-gpu-01").unwrap();
    app.wait_for("Core 1.0.0 uses data schema 8, which needs a newer Bruk")
        .unwrap();
    assert!(app.screen().shows("Fedora 41 Workstation"));

    app.press("id:machine-card-mini-lab").unwrap();
    app.wait_for("No active sessions on mini-lab.").unwrap();
    assert!(app.screen().shows("Test Connection"));

    // Open Update Cores drawer and update all suggested cores (clears win-ci-02 warning).
    app.press("id:machines-header-update-cores").unwrap();
    app.wait_for("Update Bruk Cores").unwrap();
    app.press("id:machines-update-all-cores").unwrap();
    app.wait_for(
        "5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 machine needs a newer Bruk",
    )
    .unwrap();

    // Open Add Machine drawer and connect a new remote SSH host.
    app.press("id:machines-header-add").unwrap();
    app.wait_for("Add SSH Machine").unwrap();
    app.type_text("id:machines-add-name", "ci-arm-03").unwrap();
    app.type_text("id:machines-add-ssh", "bruk@ci-arm-03")
        .unwrap();
    app.type_text("id:machines-add-address", "10.0.4.60:22")
        .unwrap();
    app.press("id:machines-add-submit").unwrap();
    app.wait_for("Added machine 'ci-arm-03' over SSH").unwrap();
    assert!(app.screen().shows(
        "6 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 machine needs a newer Bruk"
    ));
    assert!(app.screen().shows("bruk@ci-arm-03"));

    // Select build-box-01 and click Open on session #45 to jump to the live WorkflowRun view.
    app.press("id:machine-card-build-box-01").unwrap();
    app.wait_for("id:machine-session-open-45").unwrap();
    app.press("id:machine-session-open-45").unwrap();
    app.wait_for("Edit Workflow").unwrap();
    assert!(app.screen().shows("api-gateway · Run #45"));
}

#[test]
fn preferences_machines_tab_renders_reference_mockup_and_interactive_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);

    // Open Preferences via shortcut (⌘,) and navigate to the Machines tab.
    app.press_key("mod+,").unwrap();
    app.wait_for("id:prefs-tab-machines").unwrap();
    app.press("id:prefs-tab-machines").unwrap();
    app.wait_for("Ubuntu 24.04.1 LTS").unwrap();

    // Verify left rail machines list and Add Machine button match 22 PreferencesMachines.png.
    assert!(app.screen().shows("This Mac"));
    assert!(app.screen().shows("macOS 26.1 · arm64"));
    assert!(app.screen().shows("build-box-01"));
    assert!(app.screen().shows("Ubuntu 24.04 · x86_64"));
    assert!(app.screen().shows("win-ci-02"));
    assert!(app.screen().shows("Windows 11 · x86_64"));
    assert!(app.screen().shows("gpu-01"));
    assert!(app.screen().shows("Fedora 41 · x86_64"));
    assert!(app.screen().shows("mini-lab"));
    assert!(app.screen().shows("macOS 15.6 · arm64"));
    assert!(app.screen().shows("Add Machine..."));

    // Verify default selected build-box-01 detail header and all 6 sections.
    assert!(app.screen().shows("Connected"));
    assert!(app.screen().shows("Test Connection"));

    // System card
    assert!(app.screen().shows("System"));
    assert!(app.screen().shows("Operating system"));
    assert!(app.screen().shows("Ubuntu 24.04.1 LTS"));
    assert!(app.screen().shows("Kernel"));
    assert!(app.screen().shows("Linux 6.8.0-48"));
    assert!(app.screen().shows("Architecture"));
    assert!(app.screen().shows("x86_64"));
    assert!(app.screen().shows("Hardware"));
    assert!(app.screen().shows("16 cores · 64 GB memory"));
    assert!(app.screen().shows("Hostname"));

    // Access card
    assert!(app.screen().shows("Access"));
    assert!(app.screen().shows("10.0.4.21"));
    assert!(app.screen().shows("fd00::4:21"));
    assert!(app.screen().shows("22"));
    assert!(app.screen().shows("bruk"));
    assert!(app.screen().shows("None"));
    assert!(
        app.screen()
            .shows("ED25519 SHA256:Y7nR…2fKa · trusted Nov 3")
    );

    // SSH key card
    assert!(app.screen().shows("SSH key"));
    assert!(app.screen().shows("bruk-build · ED25519"));
    assert!(app.screen().shows("SHA256:q3Ff…8kPw"));
    assert!(app.screen().shows("SSH agent (macOS Keychain)"));
    assert!(
        app.screen()
            .shows("Bruk only connects with SSH keys. Password sign-in is not supported.")
    );

    // Bruk core card
    assert!(app.screen().shows("Bruk core"));
    assert!(app.screen().shows("0.8.5"));
    assert!(app.screen().shows("7 · same as this app · in sync"));
    assert!(app.screen().shows("Check Now"));
    assert!(app.screen().shows("/home/bruk/.bruk/core"));
    assert!(
        app.screen()
            .shows("Suggest when its schema is behind · ask first")
    );
    assert!(app.screen().shows(
        "Versions only matter when the data schema differs. Bruk and the core compare schemas on every connection."
    ));

    // Agents on this machine card
    assert!(app.screen().shows("Agents on this machine"));
    assert!(
        app.screen()
            .shows("/usr/local/bin/claude · isolated config · signed in inside Bruk")
    );
    assert!(
        app.screen()
            .shows("/home/bruk/.local/bin/codex · isolated config · signed in inside Bruk")
    );
    assert!(app.screen().shows(
        "The project's key is sent per session from this Mac's Keychain, never stored on the machine"
    ));

    // Work card and bottom Remove Machine action
    assert!(app.screen().shows("Work"));
    assert!(app.screen().shows("/home/bruk/work"));
    assert!(app.screen().shows("Sessions at once"));
    assert!(app.screen().shows("4"));
    assert!(
        app.screen()
            .shows("api-gateway, marketing-site, mobile-app")
    );
    assert!(app.screen().shows("Remove Machine..."));

    // Interactive flows on build-box-01:
    // 1. Test Connection & Check Now
    app.press("id:prefs-machine-test-connection").unwrap();
    app.wait_for("SSH connected to bruk@10.0.4.21 (14 ms) · Bruk core 0.8.5 (schema 7).")
        .unwrap();

    app.press("id:prefs-machine-check-core").unwrap();
    app.wait_for("Core 0.8.5 on build-box-01 matches data schema 7 — no update needed.")
        .unwrap();

    // 2. Show/Hide Host key
    app.press("id:prefs-machine-show-host-key").unwrap();
    app.wait_for("Trusted host key · ED25519 · SHA256:Y7nR…2fKa · verified Nov 3")
        .unwrap();
    app.press("id:prefs-machine-show-host-key").unwrap();

    // 3. Add a Jump host
    app.press("id:prefs-machine-edit-jump-host").unwrap();
    app.wait_for("id:prefs-inline-input").unwrap();
    app.type_text("id:prefs-inline-input", "bastion.prod.internal")
        .unwrap();
    app.press("role:button[Save Jump Host]").unwrap();
    app.wait_for("bastion.prod.internal").unwrap();

    // 4. Change SSH user
    app.press("id:prefs-machine-change-user").unwrap();
    app.clear("id:prefs-inline-input").unwrap();
    app.type_text("id:prefs-inline-input", "runner").unwrap();
    app.press("role:button[Save User]").unwrap();
    app.wait_for("Updated SSH user to 'runner'.").unwrap();

    // 5. Switch SSH key via preset
    app.press("id:prefs-machine-change-ssh-key").unwrap();
    app.wait_for("id:prefs-machine-ssh-preset-bruk-deploy")
        .unwrap();
    app.press("id:prefs-machine-ssh-preset-bruk-deploy")
        .unwrap();
    app.wait_for("bruk-deploy · ED25519").unwrap();

    // 6. Change Core updates policy
    app.press("id:prefs-machine-change-core-updates").unwrap();
    app.wait_for("id:prefs-machine-core-policy-auto").unwrap();
    app.press("id:prefs-machine-core-policy-auto").unwrap();
    app.wait_for("Update automatically when its schema is behind")
        .unwrap();

    // 7. Sign out and sign back into Claude Code on this machine
    app.press("id:prefs-machine-claude-auth").unwrap();
    app.wait_for("id:prefs-machine-claude-signout-confirm")
        .unwrap();
    app.press("id:prefs-machine-claude-signout-confirm")
        .unwrap();
    app.wait_for("/usr/local/bin/claude · isolated config · not signed in")
        .unwrap();
    app.press("id:prefs-machine-claude-auth").unwrap();
    app.wait_for("/usr/local/bin/claude · isolated config · signed in inside Bruk")
        .unwrap();

    // 8. Change Sessions at once and edit Projects
    app.press("id:prefs-machine-change-sessions").unwrap();
    app.wait_for("id:prefs-machine-sessions-preset-6").unwrap();
    app.press("id:prefs-machine-sessions-preset-6").unwrap();
    app.wait_for("Sessions at once set to 6.").unwrap();

    app.press("id:prefs-machine-edit-projects").unwrap();
    app.wait_for("id:prefs-machine-project-mobile-app").unwrap();
    app.press("id:prefs-machine-project-mobile-app").unwrap();
    app.wait_for("api-gateway, marketing-site").unwrap();

    // 9. Switch to another machine (gpu-01 & This Mac)
    app.press("id:prefs-machine-item-gpu-01").unwrap();
    app.wait_for("Fedora 41 Workstation").unwrap();
    assert!(
        app.screen()
            .shows("8 · ahead of this app (7) · not syncing")
    );

    app.press("id:prefs-machine-item-this-mac").unwrap();
    app.wait_for("Darwin 25.1.0").unwrap();
    assert!(
        app.screen()
            .shows("This Mac is the local machine and cannot be removed.")
    );

    // 10. Add a new machine and then remove it
    app.press("id:prefs-machine-add").unwrap();
    app.wait_for("id:prefs-new-machine-name").unwrap();
    app.type_text("id:prefs-new-machine-name", "staging-arm-01")
        .unwrap();
    app.type_text("id:prefs-new-machine-ipv4", "10.0.4.88")
        .unwrap();
    app.press("id:prefs-new-machine-submit").unwrap();
    app.wait_for("Added machine 'staging-arm-01'.").unwrap();
    assert!(app.screen().shows("10.0.4.88"));

    app.press("id:prefs-machine-remove").unwrap();
    app.wait_for("id:prefs-machine-remove-confirm").unwrap();
    app.press("id:prefs-machine-remove-confirm").unwrap();
    app.wait_for("Removed machine 'staging-arm-01'.").unwrap();
}

#[test]
fn menubar_popover_handles_quick_answers_and_opens_inbox_in_primary_window() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    enable_demo_mode_in_db(&db_path);

    let mut config = EngineConfig::new(factory_app::root_with_db_path_and_menubar_popover(db_path));
    config.environment.os = Os::MacOs;
    config.viewport = Size::new(px(1360.0), px(920.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("Waiting on you · 5").unwrap();

    // Popover is open as a floating popup window; Inbox lives in the primary window.
    assert!(app.window_state("menubar-popover").unwrap().is_some());
    assert!(app.window_state("inbox").unwrap().is_none());

    // Verify all 5 reference menu bar rows and footer actions from 10 AttentionSurfaces.png.
    assert!(app.screen().shows("3 agents working"));
    assert!(app.screen().shows("Run npm run lint -- --fix"));
    assert!(app.screen().shows("Install @vitest/coverage-v8"));
    assert!(app.screen().shows("Which pricing page to link?"));
    assert!(app.screen().shows("Review before pull request"));
    assert!(app.screen().shows("Loop ran out of attempts"));
    assert!(app.screen().shows("Open Inbox"));
    assert!(app.screen().shows("Pause All Agents"));
    assert!(app.screen().shows("Snooze Notifications for 1 Hour"));

    // Quick-answer the low-risk lint command directly from the menu bar popover.
    app.press("id:menubar-allow-req-8f2c").unwrap();
    app.wait_for("Waiting on you · 4").unwrap();
    assert!(!app.screen().shows("Run npm run lint -- --fix"));

    // Toggle Pause All Agents and Snooze Notifications from the popover footer.
    app.press("id:menubar-pause-agents").unwrap();
    app.wait_for("Agents paused").unwrap();
    assert!(app.screen().shows("Resume All Agents"));
    app.press("id:menubar-pause-agents").unwrap();
    app.wait_for("3 agents working").unwrap();

    app.press("id:menubar-snooze-notifications").unwrap();
    app.wait_for("Snoozed (60 min) — Resume").unwrap();

    // Clicking "Answer..." on the Claude question closes the popover and switches
    // the primary window to Inbox focused directly on that request.
    app.press("id:menubar-action-req-5a19").unwrap();
    app.wait_for("4 agents waiting · oldest 25 min · 3 still working")
        .unwrap();
    assert!(app.window_state("menubar-popover").unwrap().is_none());
    assert!(app.window_state("inbox").unwrap().is_none());
    assert!(
        app.screen()
            .shows("Which pricing page should the comparison table link to?")
    );
    assert!(app.screen().shows("Send Answer"));
}

#[test]
fn inbox_view_renders_reference_layout_and_interactive_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_demo_db(db_path);

    // Open Inbox inside the primary window via ⌘I.
    app.press_key("mod+i").unwrap();
    app.wait_for("Blocking a run · oldest first").unwrap();
    assert!(app.window_state("inbox").unwrap().is_none());

    // Verify 08 Inbox.png reference header, list items, and default selected permission detail.
    assert!(app.screen().shows("All 5"));
    assert!(app.screen().shows("Permissions"));
    assert!(app.screen().shows("Questions"));
    assert!(app.screen().shows("Reviews"));
    assert!(app.screen().shows("Pause All Agents"));
    assert!(app.screen().shows("Loop ran out of attempts"));
    assert!(app.screen().shows("Review before the pull request"));
    assert!(app.screen().shows("Claude has a question"));
    assert!(app.screen().shows("Codex wants to run a command"));
    assert!(app.screen().shows("Answered today · 6"));

    assert!(
        app.screen()
            .shows("api-gateway › Dependency upgrade › run #44 · build-box-01")
    );
    assert!(app.screen().shows("Step 2 paused · attempt 1 of 3"));
    assert!(app.screen().shows("Other runs keep going"));
    assert!(app.screen().shows("Open run"));
    assert!(app.screen().shows(
        "Coverage reporting needs the v8 provider, and it isn't installed in this project yet."
    ));
    assert!(
        app.screen()
            .shows("npm install --save-dev @vitest/coverage-v8")
    );
    assert!(app.screen().shows("Network access"));
    assert!(
        app.screen()
            .shows("Changes package.json, package-lock.json")
    );
    assert!(app.screen().shows("Not in this step's allowed commands"));
    assert!(app.screen().shows("Allow Once"));
    assert!(app.screen().shows("Allow for This Run"));
    assert!(app.screen().shows("Always Allow..."));
    assert!(app.screen().shows("Deny"));
    assert!(
        app.screen()
            .shows("Always Allow adds this rule to the Fix issue workflow")
    );
    assert!(app.screen().shows("npm install --save-dev @vitest/*"));
    assert!(app.screen().shows("Note to the agent"));
    assert!(
        app.screen()
            .shows("If unanswered by 8:05 PM, this is denied and Codex is told no one answered.")
    );
    assert!(app.screen().shows("Waiting rules"));
    assert!(app.screen().shows("Move between requests"));
    assert!(app.screen().shows("Primary answer"));

    // Toggle Waiting rules panel.
    app.press("id:inbox-waiting-rules-btn").unwrap();
    app.wait_for("• Menu bar badge turns orange and sends a silent reminder after 15 min.")
        .unwrap();
    app.press("id:inbox-waiting-rules-close").unwrap();

    // Edit command before allowing, then save an Always Allow rule for the workflow.
    app.press("id:inbox-edit-command-btn").unwrap();
    app.wait_for("id:inbox-command-input").unwrap();
    app.press("id:inbox-edit-command-btn").unwrap();

    app.press("id:inbox-always-allow-btn").unwrap();
    app.wait_for("id:inbox-save-rule-workflow").unwrap();
    app.press("id:inbox-save-rule-workflow").unwrap();
    app.wait_for("4 agents waiting · oldest 25 min · 3 still working")
        .unwrap();
    assert!(app.screen().shows("Answered today · 7"));

    // Filter to Questions and answer Claude's question.
    app.press("id:inbox-filter-questions").unwrap();
    app.wait_for("Which pricing page should the comparison table link to?")
        .unwrap();
    app.press("id:inbox-question-opt-enterprise").unwrap();
    app.press("id:inbox-send-answer").unwrap();
    app.wait_for("3 agents waiting · oldest 25 min · 3 still working")
        .unwrap();

    // Switch back to All, approve the Review gate, and inspect Answered today.
    app.press("id:inbox-filter-all").unwrap();
    app.press("id:inbox-row-req-71d0").unwrap();
    app.wait_for("Approve & Open PR").unwrap();
    assert!(app.screen().shows("src/auth/retry.ts"));
    app.press("id:inbox-approve-review").unwrap();
    app.wait_for("2 agents waiting · oldest 25 min · 3 still working")
        .unwrap();

    // Expand Answered today and inspect a completed item.
    app.press("id:inbox-answered-toggle").unwrap();
    app.wait_for("id:inbox-answered-row-req-71d0").unwrap();
    app.press("id:inbox-answered-row-req-71d0").unwrap();
    app.wait_for("Approved & opened PR").unwrap();

    // Open the underlying run in the primary window, then return to Inbox via ⌘I.
    app.press("id:inbox-open-run").unwrap();
    app.wait_for("Edit Workflow").unwrap();
    app.press_key("mod+i").unwrap();
    app.wait_for("Blocking a run · oldest first").unwrap();
}

#[test]
fn fresh_launch_without_demo_mode_shows_proper_empty_states_across_views() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path.clone());

    // Dashboard starts in a clean empty onboarding state with no fake sample data.
    assert!(app.screen().shows("Welcome to Bruk"));
    assert!(app.screen().shows("Register a Project"));
    assert!(app.screen().shows("Configure Providers…"));
    assert!(
        app.screen()
            .shows("Want to preview visuals with sample data? Enable Demo Mode in the Bruk menu.")
    );
    assert!(app.screen().shows("No projects yet"));
    assert!(app.screen().shows("None connected"));
    assert!(!app.screen().shows("api-gateway"));
    assert!(!app.screen().shows("18.4M"));
    assert!(!app.screen().shows("$63.20"));

    // Inbox shows an empty caught-up state with no fake requests or answered history.
    app.press_key("mod+i").unwrap();
    app.wait_for("No waiting requests · Idle").unwrap();
    assert!(app.screen().shows("All caught up"));
    assert!(app.screen().shows("No waiting requests"));
    assert!(!app.screen().shows("Answered today · 6"));
    assert!(!app.screen().shows("@vitest/coverage-v8"));

    // Machines shows an empty fleet state with an Add Machine call to action.
    assert!(
        app.screen()
            .matches("role:button[Machines, 0 registered]")
            .unwrap()
    );
    assert!(!app.screen().shows("0 registered"));
    app.press_key("alt+mod+m").unwrap();
    app.wait_for("No machines configured · Bruk 0.9.0 (data schema 7)")
        .unwrap();
    assert!(app.screen().shows("No machines configured"));
    assert!(app.screen().shows("No machines yet"));
    assert!(!app.screen().shows("build-box-01"));

    // Preferences shows unconfigured providers, no machines, and no fake extensions.
    app.press_key("mod+,").unwrap();
    app.wait_for("Not signed in").unwrap();
    assert!(app.screen().shows("No OpenRouter keys configured"));
    assert!(!app.screen().shows("jonathan@garbee.me"));
    assert!(!app.screen().shows("Acme team"));

    app.press("id:prefs-tab-machines").unwrap();
    app.wait_for("No machines configured").unwrap();
    assert!(!app.screen().shows("Ubuntu 24.04.1 LTS"));

    app.press("id:prefs-tab-extensions").unwrap();
    app.wait_for("No extensions installed").unwrap();
    assert!(!app.screen().shows("GitHub Issues & Pull Requests  v1.2.0"));

    app.press_window_close("preferences").unwrap();

    // Menu bar popover also shows a clean empty state when Demo Mode is disabled.
    let mut config = EngineConfig::new(factory_app::root_with_db_path_and_menubar_popover(db_path));
    config.environment.os = Os::MacOs;
    config.viewport = Size::new(px(1360.0), px(920.0));
    let mut popover_app = Headless::start(config).unwrap();
    popover_app
        .wait_for("No agents are waiting on you")
        .unwrap();
    assert!(popover_app.screen().shows("All caught up"));
    assert!(popover_app.screen().shows("No active agents"));
}

#[test]
fn registered_project_without_demo_mode_shows_clean_empty_project_views() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    let mut app = start_with_db(db_path);

    // Register a real local project without Demo Mode enabled.
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Register a local repository").unwrap();
    app.type_text("text:Project name", "my-service").unwrap();
    app.type_text("text:Repository path", "/workspace/my-service")
        .unwrap();
    app.press("role:button[Save Project]").unwrap();
    app.wait_for("Registered project 'my-service'").unwrap();

    // Dashboard now shows empty card states for the tracked project without fake telemetry.
    app.press_key("mod+shift+d").unwrap();
    app.wait_for("No token usage recorded yet").unwrap();
    assert!(app.screen().shows("my-service"));
    assert!(app.screen().shows("No provider usage recorded yet."));
    assert!(!app.screen().shows("api-gateway"));

    // Work queue for the real project shows a proper empty state with no fake issues.
    app.press("id:sidebar-work-queue-1").unwrap();
    app.wait_for("my-service · No issue source connected")
        .unwrap();
    assert!(app.screen().shows("Work queue is empty"));
    assert!(app.screen().shows("Configure Issue Source…"));
    assert!(!app.screen().shows("acme/my-service"));
    assert!(!app.screen().shows("Add health-check telemetry"));

    // Workflows overview shows an empty table with template cards ready to start.
    app.press("id:sidebar-workflows-1").unwrap();
    app.wait_for("my-service · No workflows yet").unwrap();
    assert!(app.screen().shows("No workflows yet"));
    assert!(app.screen().shows("Start from a template"));
    assert!(!app.screen().shows("Fix issue"));

    // Project Settings for the real project has no fake repo, rules, workflows, or secrets.
    app.press("id:sidebar-project-settings-1").unwrap();
    app.wait_for("my-service · Issues & intake").unwrap();
    assert!(app.window_state("project-settings").unwrap().is_none());
    assert!(app.screen().shows(
        "No intake rules configured yet. Add a rule to route labeled issues to a workflow."
    ));
    assert!(!app.screen().shows("acme/my-service"));

    app.press("id:ps-tab-workflows").unwrap();
    app.wait_for(
        "No workflow files discovered yet. Add definitions in .factory/workflows/ or create one from Workflows.",
    )
    .unwrap();
    assert!(!app.screen().shows(".factory/workflows/fix-issue.yaml"));

    app.press("id:ps-tab-environment").unwrap();
    app.wait_for("No project environment variables configured.")
        .unwrap();
    assert!(!app.screen().shows("STAGING_API_TOKEN"));

    app.press("id:ps-tab-providers").unwrap();
    app.wait_for("No OpenRouter keys configured").unwrap();
}
