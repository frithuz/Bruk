//! Headless Native Shell UI tests for everything behind the `machines` build flag.
//!
//! Build flags are process-wide, so every test in this binary turns the flag on first and tests
//! that need it off live in the other test binaries.

mod common;

use factory_flags::Flag;
use tempfile::TempDir;

#[allow(unused_imports)]
use common::*;

const DEMO_FLEET_SUBTITLE: &str = "5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Bruk";

#[test]
fn machines_view_reconnects_a_disconnected_machine() {
    factory_flags::testing::enable(Flag::Machines);
    let temp = TempDir::new().unwrap();
    let mut app = start_with_demo_db(temp.path().join("factory.db"));

    // Navigate to Machines and verify the fleet header and reconnect flow.
    app.press("Machines").unwrap();
    app.wait_for(DEMO_FLEET_SUBTITLE).unwrap();
    app.press("id:machine-card-win-ci-02").unwrap();
    app.wait_for("Windows 11 24H2").unwrap();
    app.press("id:machine-action-connection").unwrap();
    app.wait_for("Caught up just now").unwrap();
}

#[test]
fn window_menu_shortcut_opens_machines() {
    factory_flags::testing::enable(Flag::Machines);
    let temp = TempDir::new().unwrap();
    let mut app = start_with_demo_db(temp.path().join("factory.db"));

    app.press_key("alt+mod+m").unwrap();
    app.wait_for(DEMO_FLEET_SUBTITLE).unwrap();

    app.press_key("mod+shift+d").unwrap();
    app.wait_for("Issues resolved").unwrap();
}

#[test]
fn preferences_tab_strip_and_landing_picker_offer_machines() {
    factory_flags::testing::enable(Flag::Machines);
    let temp = TempDir::new().unwrap();
    let mut app = start_with_demo_db(temp.path().join("factory.db"));

    app.press_key("mod+,").unwrap();
    app.wait_for("id:prefs-tab-machines").unwrap();
    app.press("id:prefs-tab-machines").unwrap();
    app.wait_for("Ubuntu 24.04.1 LTS").unwrap();
    assert!(app.screen().shows("Test Connection"));

    app.press("id:prefs-tab-general").unwrap();
    app.wait_for("Default landing view").unwrap();
    assert!(app.screen().shows("Machines"));
}

#[test]
fn fresh_launch_without_demo_mode_shows_empty_machines_states() {
    factory_flags::testing::enable(Flag::Machines);
    let temp = TempDir::new().unwrap();
    let mut app = start_with_db(temp.path().join("factory.db"));

    assert!(app.screen().matches("id:dashboard-empty-machines").unwrap());

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

    app.press_key("mod+,").unwrap();
    app.wait_for("No Codex account configured").unwrap();
    app.press("id:prefs-tab-machines").unwrap();
    app.wait_for("No machines configured").unwrap();
    assert!(!app.screen().shows("Ubuntu 24.04.1 LTS"));
}

#[test]
fn machines_page_renders_reference_layout_and_interactive_flows() {
    factory_flags::testing::enable(Flag::Machines);
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
    factory_flags::testing::enable(Flag::Machines);
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
