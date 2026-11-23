//! Headless Native Shell UI tests for the Dashboard landing page, Inbox, Machines,
//! SQLite project tracking, and workflow execution.

use std::{ffi::OsString, path::PathBuf};

use nsh_core::{
    platform::Os,
    prelude::{Size, px},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use tempfile::TempDir;

fn start_with_db(db_path: PathBuf) -> Headless {
    start_with_db_and_claude_path(db_path, OsString::new())
}

fn start_with_db_and_claude_path(db_path: PathBuf, path_env: OsString) -> Headless {
    let mut config = EngineConfig::new(factory_app::root_with_db_path_and_claude_path(
        db_path, path_env,
    ));
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
        app.wait_for("Add a project").unwrap();
        app.press("id:register-mode-link").unwrap();
        assert!(!app.screen().enabled("role:button[Link Project]").unwrap());

        app.type_text("text:Project name", "billing-api").unwrap();
        app.type_text("text:Repository path", "/workspace/billing-api")
            .unwrap();
        app.press("role:button[Link Project]").unwrap();
        app.wait_for("Registered project 'billing-api'").unwrap();
        assert!(app.screen().shows("/workspace/billing-api"));

        // Duplicate repository path should be rejected by SQLite UNIQUE constraint.
        app.type_text("text:Project name", "billing-dup").unwrap();
        app.type_text("text:Repository path", "/workspace/billing-api")
            .unwrap();
        app.press("role:button[Link Project]").unwrap();
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
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();

    app.type_text("text:Project name", "payments").unwrap();
    app.type_text("text:Repository path", "/workspace/payments")
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
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
        app.wait_for("Run workspaces").unwrap();

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
    app.wait_for("Add a project").unwrap();

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

    // Hide and restore the inspector via View › Hide Inspector (⌥⌘I).
    assert!(app.screen().shows("Bruk will update the issue"));
    app.press_key("alt+mod+i").unwrap();
    assert!(!app.screen().shows("Bruk will update the issue"));
    app.press_key("alt+mod+i").unwrap();
    app.wait_for("Bruk will update the issue").unwrap();

    // "Edit intake rules" navigates to this project's Issues & intake Project Settings page.
    app.press("role:button[Edit intake rules]").unwrap();
    app.wait_for("api-gateway · Issues & intake").unwrap();
    assert!(app.screen().shows("Project settings"));
    assert!(app.screen().shows("Intake rules"));
    assert_eq!(
        app.screen().input_text("id:ps-github-repo").unwrap(),
        Some("acme/api-gateway")
    );

    // Switch to another project's Work queue and verify "Edit intake rules" opens that project's settings.
    app.press("role:button[marketing-site]").unwrap();
    app.wait_for("id:sidebar-work-queue-2").unwrap();
    app.press("id:sidebar-work-queue-2").unwrap();
    app.wait_for("marketing-site · GitHub Issues · acme/marketing-site")
        .unwrap();
    app.press("role:button[Edit intake rules]").unwrap();
    app.wait_for("marketing-site · Issues & intake").unwrap();
    assert!(app.screen().shows("Intake rules"));
    assert_eq!(
        app.screen().input_text("id:ps-github-repo").unwrap(),
        Some("acme/marketing-site")
    );
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
            .shows(".agents/bruk/workflows/fix-issue.yaml · Intake · 4 steps · Claude / Codex")
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
    app.wait_for("No Codex account configured").unwrap();
    assert!(app.screen().shows("No OpenRouter keys configured"));
    assert!(!app.screen().matches("role:button[Codex]").unwrap());
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
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "my-service").unwrap();
    app.type_text("text:Repository path", "/workspace/my-service")
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
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

    // Workflows overview inherits built-in catalog workflows with zero fake runs or demo items.
    app.press("id:sidebar-workflows-1").unwrap();
    app.wait_for("my-service · 2 workflows · 0 running")
        .unwrap();
    assert!(app.screen().shows("Fix issue"));
    assert!(app.screen().shows("Review a pull request"));
    assert!(app.screen().shows("Built-in"));
    assert!(!app.screen().shows("Docs refresh"));
    assert!(!app.screen().shows("Security audit"));

    // Project Settings for the real project has no fake repo, rules, or secrets,
    // and shows the built-in catalog workflows.
    app.press("id:sidebar-project-settings-1").unwrap();
    app.wait_for("my-service · Issues & intake").unwrap();
    assert!(app.window_state("project-settings").unwrap().is_none());
    assert!(app.screen().shows(
        "No intake rules configured yet. Add a rule to route labeled issues to a workflow."
    ));
    assert!(!app.screen().shows("acme/my-service"));

    app.press("id:ps-tab-workflows").unwrap();
    app.wait_for("builtin/workflows/fix-issue.yaml · Intake · 3 steps · Built-in")
        .unwrap();
    assert!(!app.screen().shows(".agents/bruk/workflows/fix-issue.yaml"));

    app.press("id:ps-tab-environment").unwrap();
    app.wait_for("No project environment variables configured.")
        .unwrap();
    assert!(!app.screen().shows("STAGING_API_TOKEN"));

    app.press("id:ps-tab-providers").unwrap();
    app.wait_for("No OpenRouter keys configured").unwrap();
    assert!(!app.screen().shows("Codex CLI"));
    assert!(!app.screen().matches("role:button[Codex]").unwrap());
}

#[cfg(unix)]
fn write_fake_claude(dir: &std::path::Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn claude_cli_on_path_and_logged_in_hides_sign_in_and_uses_system_account() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    let claude_path = write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"system-user@example.com","subscriptionType":"max","planTier":"max_5x","configDirectory":"/Users/system/.claude"}'
"#,
    );

    let mut app = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());

    // Open Preferences (⌘,); Claude is selected by default when no OpenRouter keys exist.
    app.press_key("mod+,").unwrap();
    app.wait_for("system-user@example.com").unwrap();

    assert!(app.screen().shows("CLI works"));
    assert!(app.screen().shows("Max 5x"));
    assert!(app.screen().shows(&claude_path.display().to_string()));
    assert!(app.screen().shows("/Users/system/.claude"));
    assert!(
        app.screen()
            .shows("Using your existing system Claude CLI login.")
    );
    // Authentication is proven on the system, so "Sign In" must not appear.
    assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());
    assert!(!app.screen().matches("role:button[Sign In]").unwrap());
}

#[cfg(unix)]
#[test]
fn claude_cli_missing_from_path_accepts_custom_path_and_persists_across_restart() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let custom_dir = temp.path().join("opt-claude");
    std::fs::create_dir_all(&custom_dir).unwrap();

    let custom_claude = write_fake_claude(
        &custom_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"custom-path@example.com","subscriptionType":"pro","configDirectory":"/Users/custom/.claude"}'
"#,
    );
    let custom_claude_str = custom_claude.display().to_string();

    {
        let mut app = start_with_db_and_claude_path(db_path.clone(), OsString::new());
        app.press_key("mod+,").unwrap();
        app.wait_for("Not found on default PATH").unwrap();
        assert!(app.screen().shows("CLI not found on PATH"));
        // Sign In is not shown when the binary itself is missing.
        assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());

        // Open the path editor and verify invalid paths report an error.
        app.press("id:prefs-claude-path-edit").unwrap();
        app.wait_for("id:prefs-claude-path-input").unwrap();
        app.type_text("id:prefs-claude-path-input", "/nonexistent/claude")
            .unwrap();
        app.press("id:prefs-claude-path-save").unwrap();
        app.wait_for("Claude CLI was not found at '/nonexistent/claude'.")
            .unwrap();

        // Enter the valid custom binary path; Bruk probes and proves system authentication.
        app.clear("id:prefs-claude-path-input").unwrap();
        app.type_text("id:prefs-claude-path-input", &custom_claude_str)
            .unwrap();
        app.press("id:prefs-claude-path-save").unwrap();
        app.wait_for("custom-path@example.com").unwrap();
        assert!(app.screen().shows("CLI works"));
        assert!(app.screen().shows("Pro"));
        assert!(app.screen().shows("Custom"));
        assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());
        assert!(!app.screen().matches("role:button[Sign In]").unwrap());

        app.press_window_close("preferences").unwrap();
    }

    // Reopen with an empty PATH and verify the custom path persisted and stays authenticated.
    let mut reopened = start_with_db_and_claude_path(db_path, OsString::new());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("custom-path@example.com").unwrap();
    assert!(reopened.screen().shows("CLI works"));
    assert!(reopened.screen().shows(&custom_claude_str));
    assert!(
        !reopened
            .screen()
            .matches("id:prefs-claude-sign-in")
            .unwrap()
    );

    // Clearing back to default PATH returns to the not-found state on the empty PATH.
    reopened.press("id:prefs-claude-path-edit").unwrap();
    reopened.wait_for("id:prefs-claude-path-reset").unwrap();
    reopened.press("id:prefs-claude-path-reset").unwrap();
    reopened.wait_for("Not found on default PATH").unwrap();
}

#[cfg(unix)]
#[test]
fn claude_cli_on_path_but_logged_out_shows_sign_in_and_updates_after_login() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    let state_file = temp.path().join("logged_in");
    std::fs::create_dir_all(&bin_dir).unwrap();

    let script = format!(
        r#"#!/bin/sh
STATE_FILE="{state}"
if [ "$1" = "auth" ] && [ "$2" = "login" ]; then
  touch "$STATE_FILE"
  exit 0
fi
if [ -f "$STATE_FILE" ]; then
  echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"fresh-login@example.com","subscriptionType":"max","configDirectory":"/Users/fresh/.claude"}}'
  exit 0
else
  echo '{{"loggedIn":false,"authMethod":"none","configDirectory":"/Users/fresh/.claude"}}'
  exit 1
fi
"#,
        state = state_file.display()
    );
    write_fake_claude(&bin_dir, "claude", &script);

    let mut app = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());
    app.press_key("mod+,").unwrap();
    app.wait_for("id:prefs-claude-sign-in").unwrap();
    assert!(app.screen().shows("Not signed in"));

    // Pressing Sign In runs `claude auth login` and refreshes into the authenticated state,
    // hiding the Sign In button automatically.
    app.press("id:prefs-claude-sign-in").unwrap();
    app.wait_for("fresh-login@example.com").unwrap();
    assert!(app.screen().shows("CLI works"));
    assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());
    assert!(!app.screen().matches("role:button[Sign In]").unwrap());
}

#[cfg(unix)]
#[test]
fn claude_cli_queries_available_models_and_remaining_usage_for_ui() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    let usage_counter = temp.path().join("usage_count");
    std::fs::create_dir_all(&bin_dir).unwrap();

    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"live-cli@example.com","subscriptionType":"max","planTier":"max_5x","configDirectory":"/Users/live/.claude"}}'
  exit 0
fi

# Consume stdin control requests and emit initialize + get_usage control responses.
cat >/dev/null
COUNT_FILE="{counter}"
if [ -f "$COUNT_FILE" ]; then
  SESSION_PCT=22
else
  touch "$COUNT_FILE"
  SESSION_PCT=8
fi
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"bruk_init","response":{{"models":[{{"value":"default","resolvedModel":"claude-opus-5-5","displayName":"Default (recommended)","description":"Opus 5.5"}},{{"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus 5.5","description":"For complex work"}},{{"value":"fable","resolvedModel":"claude-fable-5-2","displayName":"Fable 5.2","description":"For your toughest challenges"}},{{"value":"sonnet","resolvedModel":"claude-sonnet-5-5","displayName":"Sonnet 5.5","description":"Most efficient"}}]}}}}}}'
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"bruk_usage","response":{{"rate_limits_available":true,"rate_limits":{{"limits":[{{"kind":"session","group":"session","percent":'"$SESSION_PCT"',"resets_at":"2099-11-22T23:59:59+00:00","severity":"normal","scope":null}},{{"kind":"weekly_all","group":"weekly","percent":12,"resets_at":"2099-11-29T20:00:00+00:00","severity":"normal","scope":null}},{{"kind":"weekly_scoped","group":"weekly","percent":5,"resets_at":"2099-11-29T20:00:00+00:00","severity":"normal","scope":{{"model":{{"display_name":"Fable"}}}}}}]}}}}}}}}'
"#,
        counter = usage_counter.display()
    );
    write_fake_claude(&bin_dir, "claude", &script);

    let mut app =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    app.press_key("mod+,").unwrap();

    // Remaining usage windows and scheduled quota resets queried from the CLI appear in the card.
    app.wait_for("92% remaining").unwrap();
    assert!(app.screen().shows("Usage remaining"));
    assert!(app.screen().shows("Quota resets"));
    assert!(app.screen().shows("Current session (5h)"));
    assert!(app.screen().shows("Weekly (all models)"));
    assert!(app.screen().shows("88% remaining"));
    assert!(app.screen().shows("Weekly (Fable)"));
    assert!(app.screen().shows("95% remaining"));

    // Refresh re-queries the CLI and updates remaining usage live.
    app.press("id:prefs-claude-usage-refresh").unwrap();
    app.wait_for("78% remaining").unwrap();

    // Opening the Model editor shows the live models queried from the CLI.
    app.press("id:prefs-cli-model-change").unwrap();
    app.wait_for("Available from Claude CLI (3 models)")
        .unwrap();
    assert!(app.screen().shows("Model Family Filter"));
    assert!(app.screen().shows("Opus 5.5"));
    assert!(app.screen().shows("Fable 5.2"));
    assert!(app.screen().shows("Sonnet 5.5"));

    // Unchecking Fable in the Model Family Filter hides it from the model selection view.
    app.press("id:prefs-claude-family-fable").unwrap();
    app.wait_for("Showing 2 of 3 models from Claude CLI (Fable hidden)")
        .unwrap();
    assert!(!app.screen().shows("Fable 5.2"));
    assert!(app.screen().shows("Opus 5.5"));
    assert!(app.screen().shows("Sonnet 5.5"));

    // Re-checking Fable restores it immediately.
    app.press("id:prefs-claude-family-fable").unwrap();
    app.wait_for("Available from Claude CLI (3 models)")
        .unwrap();
    assert!(app.screen().shows("Fable 5.2"));

    // Uncheck Fable again and verify the filter persists across app restarts.
    app.press("id:prefs-claude-family-fable").unwrap();
    app.wait_for("Showing 2 of 3 models from Claude CLI (Fable hidden)")
        .unwrap();

    app.press("id:prefs-claude-model-sonnet").unwrap();
    app.press("role:button[Save Model]").unwrap();
    app.wait_for("Default model set to sonnet.").unwrap();
    assert!(app.screen().shows("sonnet"));
    app.press_window_close("preferences").unwrap();

    // Reopen against the same SQLite DB and verify Fable stays hidden.
    let mut reopened = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("Usage remaining").unwrap();
    reopened.press("id:prefs-cli-model-change").unwrap();
    reopened
        .wait_for("Showing 2 of 3 models from Claude CLI (Fable hidden)")
        .unwrap();
    assert!(!reopened.screen().shows("Fable 5.2"));
    assert!(reopened.screen().shows("Opus 5.5"));
    assert!(reopened.screen().shows("Sonnet 5.5"));
}

#[test]
fn codex_provider_is_unregistered_by_default_and_can_be_added_and_removed() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    {
        let mut app = start_with_db(db_path.clone());

        // Register a project so we can also check Project Settings › Providers.
        app.press("id:dashboard-empty-register-project").unwrap();
        app.wait_for("Add a project").unwrap();
        app.press("id:register-mode-link").unwrap();
        app.type_text("text:Project name", "my-service").unwrap();
        app.type_text("text:Repository path", "/workspace/my-service")
            .unwrap();
        app.press("role:button[Link Project]").unwrap();
        app.wait_for("Registered project 'my-service'").unwrap();

        // Project Settings › Providers must not show Codex as if it were registered.
        app.press("id:sidebar-project-settings-1").unwrap();
        app.press("id:ps-tab-providers").unwrap();
        app.wait_for("Available accounts").unwrap();
        assert!(!app.screen().shows("Codex CLI"));
        assert!(!app.screen().matches("role:button[Codex]").unwrap());

        // Preferences › Providers shows Codex as unconfigured, not as a registered account.
        app.press_key("mod+,").unwrap();
        app.wait_for("No Codex account configured").unwrap();
        assert!(!app.screen().matches("role:button[Codex]").unwrap());

        // Register Codex explicitly through Add Account.
        app.press("role:button[Add Account]").unwrap();
        app.wait_for("Add Codex CLI account...").unwrap();
        app.press("role:button[Add Codex CLI account...]").unwrap();
        app.wait_for("Add Codex CLI account").unwrap();
        app.press("id:prefs-new-codex-submit").unwrap();
        app.wait_for("Added Codex CLI account.").unwrap();
        assert!(app.screen().matches("role:button[Codex]").unwrap());
        assert!(app.screen().shows("gpt-5.3-codex"));
        app.press_window_close("preferences").unwrap();

        // Once registered, Codex appears in Project Settings › Providers.
        app.wait_for("Codex CLI").unwrap();
        assert!(app.screen().matches("role:button[Codex]").unwrap());
    }

    // Reopen and verify the registered Codex account persisted, then remove it.
    {
        let mut reopened = start_with_db(db_path.clone());
        reopened.press_key("mod+,").unwrap();
        reopened.wait_for("role:button[Codex]").unwrap();
        reopened.press("role:button[Codex]").unwrap();
        reopened.wait_for("id:prefs-codex-remove").unwrap();
        reopened.press("id:prefs-codex-remove").unwrap();
        reopened.wait_for("id:prefs-codex-confirm-remove").unwrap();
        reopened.press("id:prefs-codex-confirm-remove").unwrap();
        reopened.wait_for("No Codex account configured").unwrap();
        assert!(!reopened.screen().matches("role:button[Codex]").unwrap());
        reopened.press_window_close("preferences").unwrap();
    }

    // Reopen again and confirm Codex remains unregistered.
    let mut after_remove = start_with_db(db_path);
    after_remove.press_key("mod+,").unwrap();
    after_remove
        .wait_for("No Codex account configured")
        .unwrap();
    assert!(!after_remove.screen().matches("role:button[Codex]").unwrap());
}

#[cfg(unix)]
#[test]
fn codex_cli_queries_available_models_usage_resets_and_persists_family_filter() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    let usage_counter = temp.path().join("codex_usage_count");
    std::fs::create_dir_all(&bin_dir).unwrap();

    // Put a minimal fake `claude` alongside `codex` so Claude stays deterministic on this PATH.
    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":false,"authMethod":"none","configDirectory":"/tmp/.claude"}'
exit 1
"#,
    );

    let codex_script = format!(
        r#"#!/bin/sh
COUNT_FILE="{counter}"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      echo '{{"id":1,"result":{{"userAgent":"bruk/0.171.0","codexHome":"/Users/live/.codex"}}}}'
      ;;
    *'"method":"account/read"'*)
      echo '{{"id":2,"result":{{"account":{{"type":"chatgpt","email":"codex-live@example.com","planType":"prolite","workspaceName":null}},"requiresOpenaiAuth":true}}}}'
      ;;
    *'"method":"account/rateLimits/read"'*)
      if [ -f "$COUNT_FILE" ]; then
        SESSION_PCT=24
      else
        touch "$COUNT_FILE"
        SESSION_PCT=10
      fi
      echo '{{"id":2,"result":{{"ordinaryUsageAllowed":true,"rateLimits":{{"limitId":"codex","limitName":null,"primary":{{"usedPercent":'"$SESSION_PCT"',"windowDurationMins":300,"resetsAt":4099075199}},"secondary":{{"usedPercent":15,"windowDurationMins":10080,"resetsAt":4099665600}}}},"rateLimitsByLimitId":{{"codex":{{"limitId":"codex","limitName":null,"primary":{{"usedPercent":'"$SESSION_PCT"',"windowDurationMins":300,"resetsAt":4099075199}},"secondary":{{"usedPercent":15,"windowDurationMins":10080,"resetsAt":4099665600}}}},"codex_spark":{{"limitId":"codex_spark","limitName":"GPT-5.3-Codex-Spark","primary":{{"usedPercent":5,"windowDurationMins":10080,"resetsAt":4099665600}}}}}},"rateLimitResetCredits":{{"availableCount":2}}}}}}'
      ;;
    *'"method":"model/list"'*)
      echo '{{"id":3,"result":{{"data":[{{"id":"gpt-6.2-sol","model":"gpt-6.2-sol","displayName":"GPT-6.2-Sol","description":"Fast go-to model","hidden":false}},{{"id":"gpt-6.2-astra","model":"gpt-6.2-astra","displayName":"GPT-6.2-Astra","description":"Most capable model","hidden":false}},{{"id":"gpt-6-luna","model":"gpt-6-luna","displayName":"GPT-6-Luna","description":"Fast and affordable","hidden":false}},{{"id":"hidden-model","model":"hidden-model","displayName":"Hidden","description":"Skipped","hidden":true}}]}}}}'
      ;;
  esac
done
"#,
        counter = usage_counter.display()
    );
    write_fake_claude(&bin_dir, "codex", &codex_script);

    let mut app =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    app.press_key("mod+,").unwrap();
    app.wait_for("No Codex account configured").unwrap();

    // Register Codex explicitly through Add Account.
    app.press("role:button[Add Account]").unwrap();
    app.wait_for("Add Codex CLI account...").unwrap();
    app.press("role:button[Add Codex CLI account...]").unwrap();
    app.wait_for("Add Codex CLI account").unwrap();
    app.press("id:prefs-new-codex-submit").unwrap();

    // Codex probes system auth and queries live usage, resets, and models.
    app.wait_for("90% remaining").unwrap();
    assert!(app.screen().shows("codex-live@example.com"));
    assert!(app.screen().shows("Pro Lite"));
    assert!(app.screen().shows("/Users/live/.codex"));
    assert!(app.screen().shows("Usage remaining"));
    assert!(app.screen().shows("Quota resets"));
    assert!(app.screen().shows("Current session (5h)"));
    assert!(app.screen().shows("Weekly (all models)"));
    assert!(app.screen().shows("85% remaining"));
    assert!(app.screen().shows("Weekly (GPT-5.3-Codex-Spark)"));
    assert!(app.screen().shows("95% remaining"));
    assert!(app.screen().shows("2 rate limit reset credits available."));
    assert!(!app.screen().matches("id:prefs-codex-sign-in").unwrap());

    // Refresh re-queries the CLI and updates remaining usage live.
    app.press("id:prefs-codex-usage-refresh").unwrap();
    app.wait_for("76% remaining").unwrap();

    // Opening the Model editor shows the live Codex models queried from the CLI.
    app.press("id:prefs-cli-model-change").unwrap();
    app.wait_for("Available from Codex CLI (3 models)").unwrap();
    assert!(app.screen().shows("Model Family Filter"));
    assert!(app.screen().shows("GPT-6.2-Sol"));
    assert!(app.screen().shows("GPT-6.2-Astra"));
    assert!(app.screen().shows("GPT-6-Luna"));
    assert!(!app.screen().shows("Hidden"));

    // Unchecking Luna in the Model Family Filter hides it from the model selection view.
    app.press("id:prefs-codex-family-luna").unwrap();
    app.wait_for("Showing 2 of 3 models from Codex CLI (Luna hidden)")
        .unwrap();
    assert!(!app.screen().shows("GPT-6-Luna"));
    assert!(app.screen().shows("GPT-6.2-Sol"));
    assert!(app.screen().shows("GPT-6.2-Astra"));

    // Re-checking Luna restores it immediately.
    app.press("id:prefs-codex-family-luna").unwrap();
    app.wait_for("Available from Codex CLI (3 models)").unwrap();
    assert!(app.screen().shows("GPT-6-Luna"));

    // Uncheck Luna again, select GPT-6.2-Sol, and verify the filter persists across app restarts.
    app.press("id:prefs-codex-family-luna").unwrap();
    app.wait_for("Showing 2 of 3 models from Codex CLI (Luna hidden)")
        .unwrap();
    app.press("id:prefs-codex-model-gpt-6.2-sol").unwrap();
    app.press("role:button[Save Model]").unwrap();
    app.wait_for("Default model set to gpt-6.2-sol.").unwrap();
    app.press_window_close("preferences").unwrap();

    // Reopen against the same SQLite DB and verify Luna stays hidden and usage reloads.
    let mut reopened = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("role:button[Codex]").unwrap();
    reopened.press("role:button[Codex]").unwrap();
    reopened.wait_for("Usage remaining").unwrap();
    reopened.wait_for("76% remaining").unwrap();
    reopened.press("id:prefs-cli-model-change").unwrap();
    reopened
        .wait_for("Showing 2 of 3 models from Codex CLI (Luna hidden)")
        .unwrap();
    assert!(!reopened.screen().shows("GPT-6-Luna"));
    assert!(reopened.screen().shows("GPT-6.2-Sol"));
    assert!(reopened.screen().shows("GPT-6.2-Astra"));
}

#[cfg(unix)]
#[test]
fn codex_cli_custom_path_and_sign_in_flow() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let custom_dir = temp.path().join("opt-codex");
    let state_file = temp.path().join("codex_logged_in");
    std::fs::create_dir_all(&custom_dir).unwrap();

    let codex_script = format!(
        r#"#!/bin/sh
STATE_FILE="{state}"
if [ "$1" = "login" ]; then
  touch "$STATE_FILE"
  exit 0
fi
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      echo '{{"id":1,"result":{{"codexHome":"/Users/custom/.codex"}}}}'
      ;;
    *'"method":"account/read"'*)
      if [ -f "$STATE_FILE" ]; then
        echo '{{"id":2,"result":{{"account":{{"type":"chatgpt","email":"fresh-codex@example.com","planType":"pro"}},"requiresOpenaiAuth":true}}}}'
      else
        echo '{{"id":2,"result":{{"account":null,"requiresOpenaiAuth":true}}}}'
      fi
      ;;
    *'"method":"account/rateLimits/read"'*)
      echo '{{"id":2,"result":{{"ordinaryUsageAllowed":true,"rateLimits":{{"limitId":"codex","primary":{{"usedPercent":12,"windowDurationMins":10080,"resetsAt":4099665600}}}}}}}}'
      ;;
    *'"method":"model/list"'*)
      echo '{{"id":3,"result":{{"data":[{{"id":"gpt-6.2-sol","model":"gpt-6.2-sol","displayName":"GPT-6.2-Sol","description":"Fast go-to","hidden":false}}]}}}}'
      ;;
  esac
done
"#,
        state = state_file.display()
    );
    let custom_codex = write_fake_claude(&custom_dir, "codex", &codex_script);
    let custom_codex_str = custom_codex.display().to_string();

    let mut app = start_with_db_and_claude_path(db_path, OsString::new());
    app.press_key("mod+,").unwrap();
    app.wait_for("No Codex account configured").unwrap();

    // Register Codex when not on PATH -> shows CLI not found and Set Codex Path CTA.
    app.press("role:button[Add Account]").unwrap();
    app.wait_for("Add Codex CLI account...").unwrap();
    app.press("role:button[Add Codex CLI account...]").unwrap();
    app.wait_for("Add Codex CLI account").unwrap();
    app.press("id:prefs-new-codex-submit").unwrap();
    app.wait_for("Not found on default PATH").unwrap();
    assert!(!app.screen().matches("id:prefs-codex-sign-in").unwrap());

    // Set the custom Codex binary path; it is found but logged out, so Sign In appears.
    app.press("id:prefs-codex-path-edit").unwrap();
    app.wait_for("id:prefs-codex-path-input").unwrap();
    app.type_text("id:prefs-codex-path-input", &custom_codex_str)
        .unwrap();
    app.press("id:prefs-codex-path-save").unwrap();
    app.wait_for("id:prefs-codex-sign-in").unwrap();
    assert!(app.screen().shows("Not signed in"));

    // Pressing Sign In runs `codex login` and refreshes into authenticated + live usage state.
    app.press("id:prefs-codex-sign-in").unwrap();
    app.wait_for("fresh-codex@example.com").unwrap();
    app.wait_for("88% remaining").unwrap();
    assert!(app.screen().shows("CLI works"));
    assert!(!app.screen().matches("id:prefs-codex-sign-in").unwrap());
}

#[cfg(unix)]
#[test]
fn claude_spawn_settings_in_preferences_configure_preview_filter_persist_and_reset() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"spawn-user@example.com","subscriptionType":"max","configDirectory":"/Users/spawn/.claude"}'
"#,
    );

    {
        let mut app =
            start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
        app.press_key("mod+,").unwrap();
        app.wait_for("Spawn Settings").unwrap();
        assert!(app.screen().shows("All defaults"));

        // Collapsible <details>-style sections start collapsed so all 10 groups are scannable.
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Security"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Bare Minimal Mode"));

        // 1. Expand "Launch & CLI" collapsible section and configure CLI flags.
        app.press("id:prefs-claude-spawn-section-cli-flags")
            .unwrap();
        app.wait_for("Bare Minimal Mode").unwrap();

        app.press("id:prefs-claude-spawn-cli-permission-mode-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-cli-permission-mode-opt-acceptedits")
            .unwrap();
        app.press("id:prefs-claude-spawn-cli-permission-mode-opt-acceptedits")
            .unwrap();
        app.wait_for("1 override").unwrap();

        app.press("id:prefs-claude-spawn-cli-bare-on").unwrap();
        app.wait_for("2 overrides").unwrap();

        app.press("id:prefs-claude-spawn-cli-max-budget-usd-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-cli-max-budget-usd-input")
            .unwrap();
        app.type_text("id:prefs-claude-spawn-cli-max-budget-usd-input", "4.50")
            .unwrap();
        app.press("id:prefs-claude-spawn-cli-max-budget-usd-save")
            .unwrap();
        app.wait_for("3 overrides").unwrap();

        // Collapse Launch & CLI again and confirm its rows hide.
        app.press("id:prefs-claude-spawn-section-cli-flags")
            .unwrap();
        assert!(!app.screen().shows("Bare Minimal Mode"));

        // 2. Expand Models & Reasoning and toggle extended thinking.
        app.press("id:prefs-claude-spawn-section-models").unwrap();
        app.wait_for("Always Thinking Enabled").unwrap();
        app.press("id:prefs-claude-spawn-alwaysthinkingenabled-on")
            .unwrap();
        app.wait_for("4 overrides").unwrap();

        // 3. Expand Sandbox & Security and enable sandbox + set proxy port (with validation).
        app.press("id:prefs-claude-spawn-section-sandbox").unwrap();
        app.wait_for("Enable Bash Sandbox").unwrap();
        app.press("id:prefs-claude-spawn-sandbox-enabled-on")
            .unwrap();
        app.wait_for("5 overrides").unwrap();

        app.press("id:prefs-claude-spawn-sandbox-network-httpproxyport-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-sandbox-network-httpproxyport-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-sandbox-network-httpproxyport-input",
            "99999",
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-sandbox-network-httpproxyport-save")
            .unwrap();
        app.wait_for("invalid value for 'sandbox.network.httpProxyPort': must be at most 65535")
            .unwrap();

        app.clear("id:prefs-claude-spawn-sandbox-network-httpproxyport-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-sandbox-network-httpproxyport-input",
            "8080",
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-sandbox-network-httpproxyport-save")
            .unwrap();
        app.wait_for("6 overrides").unwrap();

        // 4. Global search hides non-matching groups and non-matching settings, and auto-expands
        // the matching group ("Context & Git") to show only "Git Commit Attribution".
        app.type_text("id:prefs-claude-spawn-search", "attribution.commit")
            .unwrap();
        app.wait_for("Git Commit Attribution").unwrap();
        assert!(app.screen().shows("Context & Git"));
        assert!(!app.screen().shows("Launch & CLI"));
        assert!(!app.screen().shows("Models & Reasoning"));
        assert!(!app.screen().shows("Sandbox & Security"));
        assert!(!app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Auto-Compact Enabled"));

        app.press("id:prefs-claude-spawn-attribution-commit-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-attribution-commit-save")
            .unwrap();
        app.press("id:prefs-claude-spawn-attribution-commit-save")
            .unwrap();
        app.wait_for("7 overrides").unwrap();
        app.press("id:prefs-claude-spawn-clear-filter").unwrap();
        app.wait_for("Launch & CLI").unwrap();

        // 5. Expand Environment & Raw, confirm DISABLE_AUTOUPDATER=1 is always enforced,
        // and add a preset env var + extra CLI args + extra JSON.
        app.press("id:prefs-claude-spawn-section-env-raw").unwrap();
        app.wait_for("Process Environment Variables").unwrap();
        assert!(app.screen().shows("DISABLE_AUTOUPDATER"));
        assert!(app.screen().shows("Always enforced"));
        app.press("id:prefs-claude-spawn-env-preset-disable-telemetry")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-env-save").unwrap();
        app.press("id:prefs-claude-spawn-env-save").unwrap();
        app.wait_for("8 overrides").unwrap();
        assert!(app.screen().shows("DISABLE_TELEMETRY"));

        app.press("id:prefs-claude-spawn-extra-args-edit").unwrap();
        app.wait_for("id:prefs-claude-spawn-extra-args-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-extra-args-input",
            "--custom-flag value",
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-extra-args-save").unwrap();
        app.wait_for("9 overrides").unwrap();

        app.press("id:prefs-claude-spawn-extra-json-edit").unwrap();
        app.wait_for("id:prefs-claude-spawn-extra-json-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-extra-json-input",
            r#"{"futureOption":true}"#,
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-extra-json-save").unwrap();
        app.wait_for("10 overrides").unwrap();

        // 6. Toggle Preview Command and confirm preview sections appear.
        app.press("id:prefs-claude-spawn-preview-toggle").unwrap();
        app.wait_for("Resolved spawn invocation").unwrap();
        assert!(app.screen().shows("Generated --settings JSON"));
        assert!(
            app.screen()
                .matches("id:prefs-claude-spawn-preview-cmd")
                .unwrap()
        );
        assert!(
            app.screen()
                .matches("id:prefs-claude-spawn-preview-json")
                .unwrap()
        );

        // 7. Toggle "Modified only" to inspect only groups & settings with active overrides
        // (hiding unmodified groups like Permissions and Interface & Session).
        app.press("id:prefs-claude-spawn-modified-only").unwrap();
        app.wait_for("Showing 10 active override(s) across 5 group(s)")
            .unwrap();
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Security"));
        assert!(app.screen().shows("Context & Git"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Permissions"));
        assert!(!app.screen().shows("Interface & Session"));
        assert!(!app.screen().shows("Auto-Compact Enabled"));

        app.press_window_close("preferences").unwrap();
    }

    // Verify persisted SQLite preferences build the exact Claude spawn plan.
    {
        let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
        let saved = db.load_preferences().unwrap().unwrap();
        assert_eq!(saved.claude_spawn_settings.override_count(), 10);
        let plan = saved
            .build_claude_spawn_plan(&factory_core::ClaudeSpawnOptions::default())
            .unwrap();
        let preview = plan.preview_command_line();
        assert!(preview.contains("DISABLE_AUTOUPDATER=1"));
        assert!(preview.contains("DISABLE_TELEMETRY=1"));
        assert!(preview.contains("--permission-mode acceptEdits"));
        assert!(preview.contains("--bare"));
        assert!(preview.contains("--max-budget-usd 4.50"));
        assert!(preview.contains("--custom-flag value"));
        let json = plan.settings_json.unwrap();
        assert_eq!(json["alwaysThinkingEnabled"], true);
        assert_eq!(json["sandbox"]["enabled"], true);
        assert_eq!(json["sandbox"]["network"]["httpProxyPort"], 8080);
        assert_eq!(json["attribution"]["commit"], "");
        assert_eq!(json["futureOption"], true);
    }

    // Reopen against the same SQLite DB and verify all 10 overrides persisted, then Reset All.
    let mut reopened =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("10 overrides").unwrap();
    reopened
        .press("id:prefs-claude-spawn-preview-toggle")
        .unwrap();
    reopened.wait_for("Resolved spawn invocation").unwrap();
    assert!(reopened.screen().shows("Generated --settings JSON"));

    reopened.press("id:prefs-claude-spawn-reset-all").unwrap();
    reopened.wait_for("All defaults").unwrap();
    assert!(!reopened.screen().shows("Generated --settings JSON"));

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let after_reset = db.load_preferences().unwrap().unwrap();
    assert!(after_reset.claude_spawn_settings.is_empty());
    let reset_plan = after_reset
        .build_claude_spawn_plan(&factory_core::ClaudeSpawnOptions::default())
        .unwrap();
    assert_eq!(
        reset_plan
            .env
            .get("DISABLE_AUTOUPDATER")
            .map(String::as_str),
        Some("1")
    );
}

#[cfg(unix)]
#[test]
fn codex_spawn_settings_in_preferences_configure_preview_filter_persist_and_reset() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":false,"authMethod":"none","configDirectory":"/tmp/.claude"}'
exit 1
"#,
    );
    write_fake_claude(
        &bin_dir,
        "codex",
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      echo '{"id":1,"result":{"userAgent":"bruk/0.171.0","codexHome":"/Users/spawn/.codex"}}'
      ;;
    *'"method":"account/read"'*)
      echo '{"id":2,"result":{"account":{"type":"chatgpt","email":"codex-spawn@example.com","planType":"pro"},"requiresOpenaiAuth":true}}'
      ;;
    *'"method":"account/rateLimits/read"'*)
      echo '{"id":2,"result":{"ordinaryUsageAllowed":true,"rateLimits":null}}'
      ;;
    *'"method":"model/list"'*)
      echo '{"id":3,"result":{"data":[]}}'
      ;;
  esac
done
"#,
    );

    {
        let mut app =
            start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
        app.press_key("mod+,").unwrap();
        app.wait_for("No Codex account configured").unwrap();

        // Register Codex so its detail pane (including Spawn Settings) is active.
        app.press("role:button[Add Account]").unwrap();
        app.wait_for("Add Codex CLI account...").unwrap();
        app.press("role:button[Add Codex CLI account...]").unwrap();
        app.wait_for("Add Codex CLI account").unwrap();
        app.press("id:prefs-new-codex-submit").unwrap();

        app.wait_for("codex-spawn@example.com").unwrap();
        app.wait_for("Spawn Settings").unwrap();
        assert!(app.screen().shows("All defaults"));

        // Collapsible <details>-style sections start collapsed so all 11 groups are scannable.
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Shell"));
        assert!(app.screen().shows("Feature Flags"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Managed Git Worktree"));

        // 1. Expand "Launch & CLI" and configure CLI flags.
        app.press("id:prefs-codex-spawn-section-cli-flags").unwrap();
        app.wait_for("Managed Git Worktree").unwrap();

        app.press("id:prefs-codex-spawn-cli-sandbox-workspace-write")
            .unwrap();
        app.wait_for("1 override").unwrap();

        app.press("id:prefs-codex-spawn-cli-worktree-on").unwrap();
        app.wait_for("2 overrides").unwrap();

        app.press("id:prefs-codex-spawn-cli-profile-edit").unwrap();
        app.wait_for("id:prefs-codex-spawn-cli-profile-input")
            .unwrap();
        app.type_text("id:prefs-codex-spawn-cli-profile-input", "fast")
            .unwrap();
        app.press("id:prefs-codex-spawn-cli-profile-save").unwrap();
        app.wait_for("3 overrides").unwrap();

        // Collapse Launch & CLI again and confirm its rows hide.
        app.press("id:prefs-codex-spawn-section-cli-flags").unwrap();
        assert!(!app.screen().shows("Managed Git Worktree"));

        // 2. Expand Models & Reasoning and configure reasoning effort.
        app.press("id:prefs-codex-spawn-section-models").unwrap();
        app.wait_for("Reasoning Effort").unwrap();
        app.press("id:prefs-codex-spawn-model-reasoning-effort-edit")
            .unwrap();
        app.wait_for("id:prefs-codex-spawn-model-reasoning-effort-opt-high")
            .unwrap();
        app.press("id:prefs-codex-spawn-model-reasoning-effort-opt-high")
            .unwrap();
        app.wait_for("4 overrides").unwrap();

        // 3. Expand Sandbox & Shell and enable network access + test validation in MCP port.
        app.press("id:prefs-codex-spawn-section-sandbox").unwrap();
        app.wait_for("Workspace-Write Network Access").unwrap();
        app.press("id:prefs-codex-spawn-sandbox-workspace-write-network-access-on")
            .unwrap();
        app.wait_for("5 overrides").unwrap();

        app.press("id:prefs-codex-spawn-section-mcp-plugins-hooks")
            .unwrap();
        app.wait_for("MCP OAuth Callback Port").unwrap();
        app.press("id:prefs-codex-spawn-mcp-oauth-callback-port-edit")
            .unwrap();
        app.wait_for("id:prefs-codex-spawn-mcp-oauth-callback-port-input")
            .unwrap();
        app.type_text(
            "id:prefs-codex-spawn-mcp-oauth-callback-port-input",
            "99999",
        )
        .unwrap();
        app.press("id:prefs-codex-spawn-mcp-oauth-callback-port-save")
            .unwrap();
        app.wait_for("invalid value for 'mcp_oauth_callback_port': must be at most 65535")
            .unwrap();

        app.clear("id:prefs-codex-spawn-mcp-oauth-callback-port-input")
            .unwrap();
        app.type_text("id:prefs-codex-spawn-mcp-oauth-callback-port-input", "8484")
            .unwrap();
        app.press("id:prefs-codex-spawn-mcp-oauth-callback-port-save")
            .unwrap();
        app.wait_for("6 overrides").unwrap();

        // 4. Global search hides non-matching groups and auto-expands Feature Flags for memories.
        app.type_text("id:prefs-codex-spawn-search", "features.memories")
            .unwrap();
        app.wait_for("Memories Feature").unwrap();
        assert!(app.screen().shows("Feature Flags"));
        assert!(!app.screen().shows("Launch & CLI"));
        assert!(!app.screen().shows("Models & Reasoning"));
        assert!(!app.screen().shows("Sandbox & Shell"));
        assert!(!app.screen().shows("Environment & Raw"));

        app.press("id:prefs-codex-spawn-features-memories-on")
            .unwrap();
        app.wait_for("7 overrides").unwrap();
        app.press("id:prefs-codex-spawn-clear-filter").unwrap();
        app.wait_for("Launch & CLI").unwrap();

        // 5. Expand Environment & Raw, confirm check_for_update_on_startup=false is always enforced,
        // and add a preset env var + extra CLI args + extra -c config override.
        app.press("id:prefs-codex-spawn-section-env-raw").unwrap();
        app.wait_for("Process Environment Variables").unwrap();
        assert!(app.screen().shows("check_for_update_on_startup"));
        assert!(app.screen().shows("Always enforced"));
        app.press("id:prefs-codex-spawn-env-preset-no-color")
            .unwrap();
        app.wait_for("id:prefs-codex-spawn-env-save").unwrap();
        app.press("id:prefs-codex-spawn-env-save").unwrap();
        app.wait_for("8 overrides").unwrap();
        assert!(app.screen().shows("NO_COLOR"));

        app.press("id:prefs-codex-spawn-extra-args-edit").unwrap();
        app.wait_for("id:prefs-codex-spawn-extra-args-input")
            .unwrap();
        app.type_text("id:prefs-codex-spawn-extra-args-input", "--ephemeral")
            .unwrap();
        app.press("id:prefs-codex-spawn-extra-args-save").unwrap();
        app.wait_for("9 overrides").unwrap();

        app.press("id:prefs-codex-spawn-extra-config-edit").unwrap();
        app.wait_for("id:prefs-codex-spawn-extra-config-input")
            .unwrap();
        app.type_text(
            "id:prefs-codex-spawn-extra-config-input",
            "future_option = true",
        )
        .unwrap();
        app.press("id:prefs-codex-spawn-extra-config-save").unwrap();
        app.wait_for("10 overrides").unwrap();

        // 6. Toggle Preview Command and confirm preview sections appear.
        app.press("id:prefs-codex-spawn-preview-toggle").unwrap();
        app.wait_for("Resolved spawn invocation").unwrap();
        assert!(app.screen().shows("Generated -c config.toml overrides"));
        assert!(
            app.screen()
                .matches("id:prefs-codex-spawn-preview-cmd")
                .unwrap()
        );
        assert!(
            app.screen()
                .matches("id:prefs-codex-spawn-preview-config")
                .unwrap()
        );

        // 7. Toggle "Modified only" to inspect only groups & settings with active overrides.
        app.press("id:prefs-codex-spawn-modified-only").unwrap();
        app.wait_for("Showing 10 active override(s) across 6 group(s)")
            .unwrap();
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Shell"));
        assert!(app.screen().shows("MCP, Plugins & Hooks"));
        assert!(app.screen().shows("Feature Flags"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Interface & TUI"));
        assert!(!app.screen().shows("Approvals & Permissions"));

        app.press_window_close("preferences").unwrap();
    }

    // Verify persisted SQLite preferences build the exact Codex spawn plan.
    {
        let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
        let saved = db.load_preferences().unwrap().unwrap();
        assert_eq!(saved.codex_spawn_settings.override_count(), 10);
        let plan = saved
            .build_codex_spawn_plan(&factory_core::CodexSpawnOptions::default())
            .unwrap();
        let preview = plan.preview_command_line();
        assert!(preview.contains("NO_COLOR=1"));
        assert!(preview.contains("-c check_for_update_on_startup=false"));
        assert!(preview.contains("--sandbox workspace-write"));
        assert!(preview.contains("--worktree"));
        assert!(preview.contains("--profile fast"));
        assert!(preview.contains("--ephemeral"));
        let config_pretty = plan.config_overrides_pretty().unwrap();
        assert!(config_pretty.contains("model_reasoning_effort = \"high\""));
        assert!(config_pretty.contains("sandbox_workspace_write.network_access = true"));
        assert!(config_pretty.contains("mcp_oauth_callback_port = 8484"));
        assert!(config_pretty.contains("features.memories = true"));
        assert!(config_pretty.contains("future_option = true"));
    }

    // Reopen against the same SQLite DB and verify all 10 overrides persisted, then Reset All.
    let mut reopened =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("role:button[Codex]").unwrap();
    reopened.press("role:button[Codex]").unwrap();
    reopened.wait_for("10 overrides").unwrap();
    reopened
        .press("id:prefs-codex-spawn-preview-toggle")
        .unwrap();
    reopened.wait_for("Resolved spawn invocation").unwrap();
    assert!(
        reopened
            .screen()
            .shows("Generated -c config.toml overrides")
    );

    reopened.press("id:prefs-codex-spawn-reset-all").unwrap();
    reopened.wait_for("All defaults").unwrap();
    assert!(
        !reopened
            .screen()
            .shows("Generated -c config.toml overrides")
    );

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let after_reset = db.load_preferences().unwrap().unwrap();
    assert!(after_reset.codex_spawn_settings.is_empty());
    let reset_plan = after_reset
        .build_codex_spawn_plan(&factory_core::CodexSpawnOptions::default())
        .unwrap();
    assert!(
        reset_plan
            .args
            .windows(2)
            .any(|w| w == ["-c", "check_for_update_on_startup=false"])
    );
}

#[test]
fn appearance_font_preferences_default_to_atkinson_and_persist_user_overrides() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    {
        let mut app = start_with_db(db_path.clone());
        app.press_key("mod+,").unwrap();
        app.wait_for("id:prefs-tab-appearance").unwrap();
        app.press("id:prefs-tab-appearance").unwrap();
        app.wait_for("UI font").unwrap();
        assert!(app.screen().shows("Code & monospace font"));
        assert!(
            app.screen().shows(
                "Default typeface for menus, labels, and app UI (Atkinson Hyperlegible Next)."
            )
        );
        assert!(app.screen().shows(
            "Monospace typeface for code, logs, commands, and keys (Atkinson Hyperlegible Mono)."
        ));

        // Verify native Font Panel picker triggers exist and are safe in headless tests.
        app.press("id:prefs-ui-font-choose").unwrap();
        app.press("id:prefs-mono-font-choose").unwrap();

        // Switch UI font to System and Code & monospace font to Custom("JetBrains Mono").
        app.press("id:prefs-ui-font-system").unwrap();
        app.press("id:prefs-mono-font-custom").unwrap();
        app.wait_for("id:prefs-mono-font-input").unwrap();
        app.type_text("id:prefs-mono-font-input", "JetBrains Mono")
            .unwrap();
        app.press("id:prefs-mono-font-apply").unwrap();

        app.press_window_close("preferences").unwrap();
    }

    // Verify persisted font overrides in SQLite.
    {
        let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
        let saved = db.load_preferences().unwrap().unwrap();
        assert_eq!(
            saved.appearance.ui_font,
            factory_core::FontPreference::System
        );
        assert_eq!(
            saved.appearance.mono_font,
            factory_core::FontPreference::Custom("JetBrains Mono".to_owned())
        );
    }

    // Reopen app and verify custom monospace font input stays populated, then switch back to Atkinson.
    let mut reopened = start_with_db(db_path.clone());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("id:prefs-tab-appearance").unwrap();
    reopened.press("id:prefs-tab-appearance").unwrap();
    reopened.wait_for("id:prefs-mono-font-input").unwrap();
    reopened.press("id:prefs-ui-font-default").unwrap();
    reopened.press("id:prefs-mono-font-default").unwrap();

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let restored = db.load_preferences().unwrap().unwrap();
    assert_eq!(
        restored.appearance.ui_font,
        factory_core::FontPreference::Default
    );
    assert_eq!(
        restored.appearance.mono_font,
        factory_core::FontPreference::Default
    );
}

#[cfg(unix)]
#[test]
fn github_intake_syncs_real_issues_persists_settings_and_updates_working_label() {
    use std::process::{Command, Stdio};

    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let repo_dir = temp.path().join("billing-api");
    let bin_dir = temp.path().join("bin");
    let gh_log = temp.path().join("gh.log");
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::create_dir_all(&bin_dir).unwrap();

    // Create a real local git repo with a GitHub origin remote so Bruk auto-detects `acme/billing-api`.
    assert!(
        Command::new("git")
            .arg("init")
            .arg(&repo_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&repo_dir)
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:acme/billing-api.git",
            ])
            .status()
            .unwrap()
            .success()
    );

    // Fake `gh` CLI in isolated PATH that logs invocations and returns realistic issue JSON.
    let gh_script = format!(
        r#"#!/bin/sh
echo "TOKEN=${{GH_TOKEN:-}} ARGS=$*" >> "{}"
if [ "$1" = "issue" ] && [ "$2" = "list" ]; then
  if [ "${{GH_TOKEN:-}}" = "ghp_invalid_token_0000" ]; then
    echo "HTTP 401: Bad credentials" >&2
    exit 1
  fi
  printf '%s\n' '[{{"number":101,"title":"Retry webhook timeouts","body":"Add jittered retry for 503s.","labels":[{{"name":"agent-ready"}},{{"name":"bug"}}],"url":"https://github.com/acme/billing-api/issues/101","state":"OPEN"}},{{"number":102,"title":"Audit billing metrics","body":"Verify counters match invoices.","labels":[{{"name":"agent-ready"}}],"url":"https://github.com/acme/billing-api/issues/102","state":"OPEN"}},{{"number":103,"title":"Explore multi-currency support","body":"Draft RFC.","labels":[{{"name":"rfc"}}],"url":"https://github.com/acme/billing-api/issues/103","state":"OPEN"}}]'
  exit 0
fi
exit 0
"#,
        gh_log.display()
    );
    write_fake_claude(&bin_dir, "gh", &gh_script);

    {
        let mut app =
            start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_owned());

        // Register the real git repository.
        app.press("id:dashboard-empty-register-project").unwrap();
        app.wait_for("Add a project").unwrap();
        app.press("id:register-mode-link").unwrap();
        app.type_text("text:Project name", "billing-api").unwrap();
        app.type_text("text:Repository path", &repo_dir.display().to_string())
            .unwrap();
        app.press("role:button[Link Project]").unwrap();
        app.wait_for("Registered project 'billing-api'").unwrap();

        // Project Settings should auto-detect `acme/billing-api` from git origin.
        app.press("id:sidebar-project-settings-1").unwrap();
        app.wait_for("billing-api · Issues & intake").unwrap();
        assert_eq!(
            app.screen().input_text("id:ps-github-repo").unwrap(),
            Some("acme/billing-api")
        );
        assert!(app.screen().shows("Ready to verify · press Test & Sync"));

        // Configure a `label:agent-ready` search filter, a `bug -> Fix issue` rule, and a fallback workflow (`Triage new issues`).
        app.press("id:ps-add-filter-btn").unwrap();
        app.wait_for("id:ps-filter-value").unwrap();
        app.type_text("id:ps-filter-value", "agent-ready").unwrap();
        app.press("id:ps-filter-save").unwrap();
        app.wait_for("Query: label:agent-ready").unwrap();

        app.type_text("id:ps-fallback-workflow", "Triage new issues")
            .unwrap();
        app.press("id:ps-add-rule-btn").unwrap();
        app.wait_for("id:ps-new-rule-label").unwrap();
        app.type_text("id:ps-new-rule-label", "bug").unwrap();
        app.press("id:ps-rule-save").unwrap();
        app.wait_for("has label bug").unwrap();

        // Trigger real GitHub sync via `Test & Sync`.
        app.press("id:ps-github-sync-btn").unwrap();
        app.wait_for("Connected · 3 open issues · synced just now")
            .unwrap();

        // Navigate to Work queue and verify synced issues were routed cleanly.
        app.press("id:sidebar-work-queue-1").unwrap();
        app.wait_for("billing-api · GitHub Issues · acme/billing-api")
            .unwrap();
        assert!(app.screen().shows("Retry webhook timeouts"));
        assert!(app.screen().shows("Audit billing metrics"));
        assert!(app.screen().shows("label bug → Fix issue"));
        assert!(
            app.screen()
                .shows("1 open issue doesn't match the intake rules")
        );

        // Start #101 -> should invoke `gh issue edit 101 --repo acme/billing-api --add-label agent-working`.
        app.press("id:wq-inspector-start-now").unwrap();
        app.wait_for("Started issue #101 as run #1").unwrap();
    }

    // Confirm `gh` received both `issue list` and `issue edit --add-label agent-working`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let log = std::fs::read_to_string(&gh_log).unwrap_or_default();
        if log.contains("issue edit 101 --repo acme/billing-api --add-label agent-working") {
            assert!(log.contains("issue list --repo acme/billing-api --state open"));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected gh add-label call in log, got:\n{log}"
        );
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    // Reopen the app against the same SQLite DB: settings and synced issues must persist.
    {
        let mut reopened =
            start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_owned());
        reopened.press("id:sidebar-work-queue-1").unwrap();
        reopened
            .wait_for("billing-api · GitHub Issues · acme/billing-api")
            .unwrap();
        assert!(reopened.screen().shows("Retry webhook timeouts"));
        assert!(reopened.screen().shows("Audit billing metrics"));

        // Switch to an invalid PAT and verify auth failure surfaces cleanly.
        reopened.press("id:sidebar-project-settings-1").unwrap();
        reopened.wait_for("billing-api · Issues & intake").unwrap();
        reopened.press("id:ps-use-token-btn").unwrap();
        reopened.wait_for("id:ps-token-input").unwrap();
        reopened
            .type_text("id:ps-token-input", "ghp_invalid_token_0000")
            .unwrap();
        reopened.press("id:ps-token-save").unwrap();
        reopened
            .wait_for("Personal access token (ghp_••••••••••••0000)")
            .unwrap();

        reopened.press("id:ps-github-sync-btn").unwrap();
        reopened
            .wait_for("Sync error · GitHub authentication failed: HTTP 401: Bad credentials")
            .unwrap();

        // Error banner also appears on the Work queue view.
        reopened.press("id:sidebar-work-queue-1").unwrap();
        reopened.wait_for("id:wq-sync-error-banner").unwrap();
    }
}

#[cfg(unix)]
#[test]
fn github_intake_supports_optional_assignee_and_search_filter_rules() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    // Fake `gh` CLI returning issues with assignees, milestones, and labels (no `agent-ready` label).
    let gh_script = r#"#!/bin/sh
if [ "$1" = "issue" ] && [ "$2" = "list" ]; then
  printf '%s\n' '[{"number":501,"title":"Fix OAuth token rotation","body":"Assigned to bot.","labels":[{"name":"bug"}],"assignees":[{"login":"factory-bot"}],"author":{"login":"alice"},"milestone":{"title":"v2.0"},"url":"https://github.com/acme/filters-demo/issues/501","state":"OPEN"},{"number":502,"title":"Update onboarding docs","body":"Assigned to bot without bug label.","labels":[],"assignees":[{"login":"factory-bot"}],"author":{"login":"alice"},"milestone":null,"url":"https://github.com/acme/filters-demo/issues/502","state":"OPEN"},{"number":503,"title":"Human-owned roadmap item","body":"Not assigned to bot.","labels":[{"name":"bug"}],"assignees":[{"login":"bob"}],"author":{"login":"bob"},"milestone":null,"url":"https://github.com/acme/filters-demo/issues/503","state":"OPEN"}]'
  exit 0
fi
exit 0
"#;
    write_fake_claude(&bin_dir, "gh", gh_script);

    let mut app = start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_owned());

    // Register a project and open Issues & intake settings.
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "filters-demo").unwrap();
    app.type_text("text:Repository path", "/workspace/filters-demo")
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'filters-demo'").unwrap();

    app.press("id:sidebar-project-settings-1").unwrap();
    app.wait_for("filters-demo · Issues & intake").unwrap();

    // Configure repo and add an optional assignee search filter (`assignee:factory-bot`).
    app.type_text("id:ps-github-repo", "acme/filters-demo")
        .unwrap();

    app.press("id:ps-add-filter-btn").unwrap();
    app.wait_for("id:ps-filter-value").unwrap();
    app.press("id:ps-filter-qualifier").unwrap();
    app.press("role:option[Assigned to]").unwrap();
    app.type_text("id:ps-filter-value", "@factory-bot").unwrap();
    app.press("id:ps-filter-save").unwrap();
    app.wait_for("Query: assignee:factory-bot").unwrap();

    // Add a label rule (`has label bug -> Fix issue`) and a fallback workflow (`Agent Triage`).
    app.type_text("id:ps-fallback-workflow", "Agent Triage")
        .unwrap();
    app.press("id:ps-add-rule-btn").unwrap();
    app.wait_for("id:ps-new-rule-label").unwrap();
    app.type_text("id:ps-new-rule-label", "bug").unwrap();
    app.press("id:ps-rule-save").unwrap();
    app.wait_for("has label bug").unwrap();

    // Sync via GitHub CLI and verify #501 + #502 (assigned to @factory-bot) are queued while #503 (assigned to @bob) is unmatched.
    app.press("id:ps-github-sync-btn").unwrap();
    app.wait_for("Connected · 3 open issues · synced just now")
        .unwrap();

    app.press("id:sidebar-work-queue-1").unwrap();
    app.wait_for("filters-demo · GitHub Issues · acme/filters-demo")
        .unwrap();
    assert!(app.screen().shows("Fix OAuth token rotation"));
    assert!(app.screen().shows("Update onboarding docs"));
    assert!(
        app.screen()
            .shows("1 open issue doesn't match the intake rules")
    );
    app.press("id:wq-unmatched-toggle").unwrap();
    app.wait_for("Not assigned to @factory-bot").unwrap();
}

#[test]
fn real_project_loads_layered_agents_bruk_catalog_and_copy_on_edit_saves_yaml() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let repo_dir = temp.path().join("catalog-repo");
    let module_dir = temp.path().join("acme-shared");

    std::fs::create_dir_all(module_dir.join("workflows")).unwrap();
    std::fs::write(
        module_dir.join("workflows/security-scan.yaml"),
        "id: security-scan\nname: Security Scan\ndescription: Company security check\nnodes:\n  - kind: agent_task\n    id: audit\n    name: Security review\n    agent: reviewer\n    prompt: Audit diff\n",
    )
    .unwrap();

    let bruk_dir = repo_dir.join(".agents/bruk");
    std::fs::create_dir_all(&bruk_dir).unwrap();
    std::fs::write(
        bruk_dir.join("bruk.yaml"),
        format!(
            "schema_version: 1\nmodules:\n  - name: acme\n    source:\n      path: {}\n",
            module_dir.display()
        ),
    )
    .unwrap();

    let mut app = start_with_db(db_path);
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "catalog-repo").unwrap();
    app.type_text("text:Repository path", &repo_dir.display().to_string())
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'catalog-repo'").unwrap();

    // Workflows overview shows Built-in workflows + Module workflow with layer badges.
    app.press("id:sidebar-workflows-1").unwrap();
    app.wait_for("catalog-repo · 3 workflows · 0 running")
        .unwrap();
    assert!(app.screen().shows("Fix issue"));
    assert!(app.screen().shows("Built-in"));
    assert!(app.screen().shows("Security Scan"));
    assert!(app.screen().shows("Module · acme"));

    // Open Built-in `Fix issue` in editor, validate, and Save -> copy-on-edit writes `.agents/bruk/workflows/fix-issue.yaml`.
    app.press("id:wfo-row-fix-issue").unwrap();
    app.press("id:wfo-inspector-edit").unwrap();
    app.wait_for("id:wfe-origin-badge").unwrap();
    assert!(app.screen().shows("Built-in"));

    app.press("id:wfe-header-validate").unwrap();
    app.wait_for("Valid workflow — 3 steps, ready to run.")
        .unwrap();

    app.press("id:wfe-header-save").unwrap();
    app.wait_for("Saved project override 'Fix issue' to .agents/bruk/workflows/fix-issue.yaml")
        .unwrap();
    assert!(
        repo_dir
            .join(".agents/bruk/workflows/fix-issue.yaml")
            .is_file()
    );

    app.press("id:wfe-header-back").unwrap();
    app.wait_for("catalog-repo · 3 workflows · 0 running")
        .unwrap();
    assert!(app.screen().shows("Project"));
}

#[test]
fn real_workflow_run_executes_shell_and_approval_gate_via_inbox_and_persists_to_sqlite() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let repo_dir = temp.path().join("runner-repo");

    std::fs::create_dir_all(repo_dir.join(".agents/bruk/workflows")).unwrap();
    std::fs::write(
        repo_dir.join(".agents/bruk/workflows/shell-and-approve.yaml"),
        "id: shell-and-approve\nname: Shell And Approve\ndescription: Runs a real shell command then waits in Inbox\ntriggers:\n  work_queue: false\n  ad_hoc: true\nnodes:\n  - kind: shell_command\n    id: hello\n    name: Run check\n    command: echo 'hello from real runner'\n  - kind: approval_gate\n    id: gate\n    name: Final approval\n    prompt: Please approve the real run\n",
    )
    .unwrap();

    let mut app = start_with_db(db_path.clone());
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "runner-repo").unwrap();
    app.type_text("text:Repository path", &repo_dir.display().to_string())
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'runner-repo'").unwrap();

    // Launch real ad-hoc run from Workflows Overview.
    app.press("id:sidebar-workflows-1").unwrap();
    app.wait_for("Shell And Approve").unwrap();
    app.press("id:wfo-row-shell-and-approve").unwrap();
    app.press("id:wfo-inspector-adhoc").unwrap();
    app.wait_for("id:wfo-adhoc-input").unwrap();
    app.type_text("id:wfo-adhoc-input", "Verify end-to-end runner")
        .unwrap();
    app.press("id:wfo-adhoc-start").unwrap();

    // Runner executes the shell step and pauses on the Approval Gate, posting to Inbox.
    app.wait_for("Review required").unwrap();
    app.press("id:sidebar-inbox-btn").unwrap();
    app.wait_for("Final approval").unwrap();
    app.wait_for("Please approve the real run").unwrap();

    // Approve directly from Inbox -> unblocks the runner and completes the workflow!
    app.press("id:inbox-approve-review").unwrap();
    app.wait_for("Completed 'Shell And Approve' (2 steps)")
        .unwrap();

    // Verify persistence in SQLite.
    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let runs = db.list_all_workflow_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].workflow_id, "shell-and-approve");
    assert_eq!(runs[0].status, factory_core::RunExecutionState::Succeeded);
}

fn isolated_home_environment(temp: &std::path::Path) -> factory_core::HomeEnvironment {
    factory_core::HomeEnvironment {
        env_override: None,
        default_root: temp.join("home").join("Bruk"),
        locator_path: temp.join("support").join("Bruk").join("location"),
        legacy_database_paths: vec![
            temp.join("support")
                .join("software-factory")
                .join("factory.db"),
        ],
    }
}

fn start_with_home_environment(environment: factory_core::HomeEnvironment) -> Headless {
    let mut config = EngineConfig::new(factory_app::root_with_home_environment(
        environment,
        OsString::new(),
    ));
    config.environment.os = Os::MacOs;
    config.viewport = Size::new(px(1360.0), px(920.0));
    Headless::start(config).unwrap()
}

#[test]
fn first_launch_sets_up_custom_bruk_folder_imports_legacy_data_and_remembers_it() {
    let temp = TempDir::new().unwrap();
    let environment = isolated_home_environment(temp.path());
    let legacy_db = environment.legacy_database_paths[0].clone();
    {
        let db = factory_core::ProjectDatabase::open(&legacy_db).unwrap();
        db.register_project("legacy-api", "/workspace/legacy-api", "main", "")
            .unwrap();
    }
    let custom_root = temp.path().join("Data").join("Bruk");

    {
        let mut app = start_with_home_environment(environment.clone());
        app.wait_for("Welcome to Bruk").unwrap();
        assert!(
            app.screen()
                .shows("Import existing projects and preferences")
        );
        assert!(app.screen().shows("Create Bruk Folder"));

        app.clear("id:home-setup-root").unwrap();
        app.type_text("id:home-setup-root", &custom_root.display().to_string())
            .unwrap();
        app.press("id:home-setup-continue").unwrap();
        app.wait_for(&format!(
            "Bruk folder ready at {} · imported projects and preferences from {}",
            factory_core::display_with_tilde(&custom_root),
            factory_core::display_with_tilde(&legacy_db),
        ))
        .unwrap();
        app.wait_for("legacy-api").unwrap();
        assert!(!app.screen().shows("Welcome to Bruk"));
    }

    assert!(custom_root.join("bruk.db").is_file());
    assert!(custom_root.join("projects").is_dir());
    assert!(custom_root.join("runs").is_dir());
    assert_eq!(
        std::fs::read_to_string(&environment.locator_path)
            .unwrap()
            .trim(),
        custom_root.display().to_string()
    );
    assert!(!environment.default_root.exists());
    assert!(legacy_db.is_file(), "legacy database must be left in place");

    // Relaunching opens the remembered folder directly.
    let mut reopened = start_with_home_environment(environment);
    reopened.wait_for("legacy-api").unwrap();
    assert!(!reopened.screen().shows("Welcome to Bruk"));
}

#[test]
fn first_launch_default_folder_without_legacy_data_skips_import() {
    let temp = TempDir::new().unwrap();
    let environment = isolated_home_environment(temp.path());

    let mut app = start_with_home_environment(environment.clone());
    app.wait_for("Welcome to Bruk").unwrap();
    assert!(
        !app.screen()
            .shows("Import existing projects and preferences")
    );
    app.press("id:home-setup-continue").unwrap();
    app.wait_for("Dashboard").unwrap();

    assert!(environment.default_root.join("bruk.db").is_file());
    assert!(
        !environment.locator_path.exists(),
        "the default folder needs no locator file"
    );
}

/// Creates a git repository with one commit on `trunk` to clone from.
fn init_origin_repo(origin: &std::path::Path) -> PathBuf {
    use std::process::{Command, Stdio};

    std::fs::create_dir_all(origin).unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(origin)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "git {args:?} failed"
        );
    };
    git(&["init", "-b", "trunk"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(origin.join("README.md"), "# billing-api\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "init"]);
    origin.to_path_buf()
}

#[test]
fn clones_repository_into_bruk_folder_registers_it_and_keeps_clone_on_remove() {
    let temp = TempDir::new().unwrap();
    let home_root = temp.path().join("Bruk");
    let db_path = home_root.join("bruk.db");
    let origin = init_origin_repo(&temp.path().join("origin").join("billing-api"));

    let clone = home_root.join("projects").join("billing-api");
    let origin_str = origin.display().to_string();

    let mut app = start_with_db(db_path.clone());
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    assert!(
        !app.screen()
            .enabled("role:button[Clone Repository]")
            .unwrap()
    );

    // The project name follows the repository name and previews the destination.
    app.type_text("id:register-clone-source", &origin_str)
        .unwrap();
    app.wait_for(&format!(
        "Clones into {}",
        factory_core::display_with_tilde(&clone)
    ))
    .unwrap();
    app.press("role:button[Clone Repository]").unwrap();
    app.wait_for("Registered project 'billing-api'").unwrap();
    assert!(app.screen().shows("In Bruk folder"));
    assert!(clone.join("README.md").is_file());

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let projects = db.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].root_path, clone);
    assert_eq!(projects[0].default_branch, "trunk");

    // Cloning into an occupied folder is refused with guidance.
    app.type_text("id:register-clone-source", &origin_str)
        .unwrap();
    app.press("role:button[Clone Repository]").unwrap();
    app.wait_for(&format!(
        "'{}' already exists. Link it as an existing clone or choose another project name.",
        clone.display()
    ))
    .unwrap();

    // Removing the project leaves the clone on disk.
    app.press("role:button[Remove]").unwrap();
    app.wait_for(&format!(
        "Removed project 'billing-api'. Its clone is still at {}.",
        factory_core::display_with_tilde(&clone)
    ))
    .unwrap();
    assert!(clone.join("README.md").is_file());
}

#[test]
fn default_projects_dir_follows_bruk_folder_and_redirects_clones_when_changed() {
    let temp = TempDir::new().unwrap();
    let home_root = temp.path().join("Bruk");
    let db_path = home_root.join("bruk.db");
    let origin = init_origin_repo(&temp.path().join("origin").join("billing-api"));
    let origin_str = origin.display().to_string();
    let custom_dir = temp.path().join("src");
    std::fs::create_dir_all(&custom_dir).unwrap();
    let saved_projects_dir = || {
        factory_core::ProjectDatabase::open(&db_path)
            .unwrap()
            .load_preferences()
            .unwrap()
            .map(|prefs| prefs.general.default_projects_dir)
    };

    let mut app = start_with_db(db_path.clone());

    // By default, clones land in the Bruk folder's projects/.
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.wait_for(&format!(
        "Clones into {}/",
        factory_core::display_with_tilde(&home_root.join("projects"))
    ))
    .unwrap();

    // Point the default projects directory somewhere else in Preferences › General.
    app.press_key("mod+,").unwrap();
    app.press("id:prefs-tab-general").unwrap();
    app.wait_for("Default projects directory").unwrap();
    app.clear("id:prefs-default-projects-dir").unwrap();
    app.type_text(
        "id:prefs-default-projects-dir",
        &custom_dir.display().to_string(),
    )
    .unwrap();
    app.press("role:button[Apply]").unwrap();
    assert_eq!(
        saved_projects_dir().as_deref(),
        Some(custom_dir.display().to_string().as_str())
    );
    app.press_window_close("preferences").unwrap();

    // Clones now go to the custom directory.
    app.type_text("id:register-clone-source", &origin_str)
        .unwrap();
    let custom_clone = custom_dir.join("billing-api");
    app.wait_for(&format!(
        "Clones into {}",
        factory_core::display_with_tilde(&custom_clone)
    ))
    .unwrap();
    app.press("role:button[Clone Repository]").unwrap();
    app.wait_for("Registered project 'billing-api'").unwrap();
    assert!(custom_clone.join("README.md").is_file());
    assert!(!home_root.join("projects").join("billing-api").exists());

    // Clearing the field goes back to following the Bruk folder.
    app.press_key("mod+,").unwrap();
    app.press("id:prefs-tab-general").unwrap();
    app.wait_for("Default projects directory").unwrap();
    app.clear("id:prefs-default-projects-dir").unwrap();
    app.press("role:button[Apply]").unwrap();
    assert_eq!(saved_projects_dir().as_deref(), Some(""));
    app.press_window_close("preferences").unwrap();
    app.wait_for(&format!(
        "Clones into {}/",
        factory_core::display_with_tilde(&home_root.join("projects"))
    ))
    .unwrap();
}
