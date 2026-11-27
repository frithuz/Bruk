//! Headless Native Shell UI tests for the Dashboard, Inbox, Machines, Menus, and empty states.

mod common;

use nsh_core::{
    platform::Os,
    prelude::{Size, px},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use tempfile::TempDir;

#[allow(unused_imports)]
use common::*;

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
    assert!(app.screen().shows("Highest token runs & chats"));
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
    assert!(!app.screen().shows("None connected"));
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
    assert!(
        !app.screen()
            .matches("id:sidebar-project-settings-1")
            .unwrap()
    );
    app.press_key("mod+shift+,").unwrap();
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
