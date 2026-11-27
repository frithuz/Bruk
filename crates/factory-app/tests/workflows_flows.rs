//! Headless Native Shell UI tests for workflows, work queue, project settings, and GitHub intake.

mod common;

use tempfile::TempDir;

#[allow(unused_imports)]
use common::*;

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

    // Verify top header, sidebar extensions, and footer status bar.
    assert!(app.screen().shows("Release prep"));
    assert!(app.screen().shows("Search or run a command"));
    assert!(app.screen().shows("Edit Workflow"));
    assert!(app.screen().shows("Deploy Watch"));
    assert!(app.screen().shows("2.7.3"));
    assert!(app.screen().shows("2.7.2"));
    assert!(app.screen().shows("Run history"));
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
