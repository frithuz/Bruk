//! Native macOS application menu bar (`Bruk`, `File`, `Edit`, `View`,
//! `Project`, `Workflow`, `Run`, `Window`, `Help`).
//!
//! Implemented commands are wired to live application state and native AppKit
//! actions; unimplemented commands remain visible as disabled entries matching
//! the reference menu specification.

use factory_core::{
    PreferencesTab, ProjectId, ProjectSettingsTab, RunDetailTab, RunExecutionState, RunStatus,
    StepTypeKind,
};
use nsh_core::{
    desktop::{MenuRole, WindowMode},
    element::{
        desktop::{MenuItem, main_window, set_application_menu},
        reveal_path,
    },
    file_dialog::OpenDialog,
    prelude::*,
};

use crate::store;

/// Inline SVG puzzle-piece template icon for "From extensions" section headers.
const EXTENSIONS_ICON_URI: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyMCAyMCIgZmlsbD0ibm9uZSIgc3Ryb2tlPSJibGFjayIgc3Ryb2tlLXdpZHRoPSIxLjUiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCI+PHBhdGggZD0iTTcuNSA0LjJhMS43IDEuNyAwIDAgMSAzLjQgMGgzLjNhMS4zIDEuMyAwIDAgMSAxLjMgMS4zdjMuM2ExLjcgMS43IDAgMCAxIDAgMy40djMuM2ExLjMgMS4zIDAgMCAxLTEuMyAxLjNINS41YTEuMyAxLjMgMCAwIDEtMS4zLTEuM1Y1LjVhMS4zIDEuMyAwIDAgMSAxLjMtMS4zaDJaIi8+PC9zdmc+";

/// Minimal reactive slice of application state that affects the menu bar.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuSnapshot {
    demo_mode: bool,
    sidebar_visible: bool,
    is_full_screen: bool,
    recent_projects: Vec<(ProjectId, String)>,
    has_selected_project: bool,
    on_work_queue: bool,
    on_workflows: bool,
    on_workflow_editor: bool,
    on_workflow_run: bool,
    inspector_visible: bool,
    has_queued_issues: bool,
    can_pause_run: bool,
    workflow_run_paused: bool,
    can_stop_run: bool,
    can_approve_step: bool,
    has_projects: bool,
    updater_state: crate::updater::UpdaterState,
}

fn current_menu_snapshot() -> MenuSnapshot {
    let demo_mode = store::demo_mode();
    let sidebar_visible = store::sidebar_visible();
    let is_full_screen = main_window()
        .state()
        .is_some_and(|state| state.mode == WindowMode::FullScreen);
    let recent_projects = store::recent_projects();
    let has_projects = !recent_projects.is_empty();
    let has_selected_project = store::selected_project().is_some();
    let active_view = store::active_view();
    let on_work_queue = active_view == store::ActiveView::WorkQueue;
    let on_workflows = active_view == store::ActiveView::Workflows;
    let on_workflow_editor = active_view == store::ActiveView::WorkflowEditor;
    let on_workflow_run = active_view == store::ActiveView::WorkflowRun;
    let inspector_visible = if on_workflow_run {
        store::workflow_run_inspector_visible()
    } else if on_workflow_editor {
        store::workflow_editor_inspector_visible()
    } else if on_workflows {
        store::workflows_inspector_visible()
    } else {
        store::work_queue_inspector_visible()
    };
    let has_queued_issues = store::has_queued_work_issues();
    let (can_pause_run, workflow_run_paused, can_stop_run, can_approve_step) = if on_workflow_run {
        let run = store::active_workflow_run();
        let can_pause = matches!(
            run.execution_state,
            RunExecutionState::Running | RunExecutionState::Paused
        );
        let paused = run.execution_state == RunExecutionState::Paused;
        let can_stop = !run.execution_state.is_terminal();
        let can_approve = !run.execution_state.is_terminal();
        (can_pause, paused, can_stop, can_approve)
    } else {
        let selected_run = store::selected_run();
        let can_stop = selected_run
            .as_ref()
            .is_some_and(|run| !run.status.is_terminal());
        let can_approve = active_view != store::ActiveView::Inbox
            && selected_run
                .as_ref()
                .is_some_and(|run| run.status == RunStatus::WaitingForApproval);
        (false, false, can_stop, can_approve)
    };

    MenuSnapshot {
        demo_mode,
        sidebar_visible,
        is_full_screen,
        recent_projects,
        has_selected_project,
        on_work_queue,
        on_workflows,
        on_workflow_editor,
        on_workflow_run,
        inspector_visible,
        has_queued_issues,
        can_pause_run,
        workflow_run_paused,
        can_stop_run,
        can_approve_step,
        has_projects,
        updater_state: store::updater_state(),
    }
}

/// Installs the reactive macOS application menu bar for the root app surface.
pub(crate) fn install_application_menu() {
    let snapshot = computed(current_menu_snapshot);
    effect(move || {
        let state = snapshot.get();
        set_application_menu(build_application_menu(&state));
    });
}

fn build_application_menu(snapshot: &MenuSnapshot) -> Vec<MenuItem> {
    vec![
        app_menu(snapshot),
        file_menu(snapshot),
        edit_menu(),
        view_menu(snapshot),
        project_menu(snapshot),
        workflow_menu(snapshot),
        run_menu(snapshot),
        window_menu(),
        help_menu(),
    ]
}

// --- Top-Level Menus --------------------------------------------------------------------

fn app_menu(snapshot: &MenuSnapshot) -> MenuItem {
    MenuItem::submenu(
        "Bruk",
        [
            MenuItem::role(MenuRole::About),
            update_menu_item(&snapshot.updater_state),
            MenuItem::separator(),
            MenuItem::new("Settings…")
                .shortcut("mod+,")
                .on_select(store::open_preferences),
            MenuItem::new("Extensions…")
                .on_select(|| store::open_preferences_tab(PreferencesTab::Extensions)),
            MenuItem::separator(),
            MenuItem::new("Demo Mode")
                .checked(snapshot.demo_mode)
                .on_select(store::toggle_demo_mode),
            MenuItem::separator(),
            MenuItem::submenu("Services", []).disabled(true),
            MenuItem::separator(),
            MenuItem::role(MenuRole::Hide),
            MenuItem::role(MenuRole::HideOthers),
            MenuItem::role(MenuRole::ShowAll),
            MenuItem::separator(),
            MenuItem::role(MenuRole::Quit),
        ],
    )
}

fn file_menu(snapshot: &MenuSnapshot) -> MenuItem {
    let recent_menu = if snapshot.recent_projects.is_empty() {
        MenuItem::submenu("Open Recent", []).disabled(true)
    } else {
        let items: Vec<MenuItem> = snapshot
            .recent_projects
            .iter()
            .map(|(id, name)| {
                let project_id = *id;
                MenuItem::new(name.clone()).on_select(move || {
                    store::select_project(project_id);
                    main_window().focus();
                })
            })
            .collect();
        MenuItem::submenu("Open Recent", items)
    };

    MenuItem::submenu(
        "File",
        [
            MenuItem::new("New Workflow")
                .shortcut("mod+n")
                .disabled(!snapshot.has_projects)
                .on_select(open_new_workflow),
            MenuItem::new("New Chat")
                .shortcut("mod+t")
                .disabled(!snapshot.has_projects)
                .on_select(open_new_chat),
            MenuItem::new("New Project…")
                .shortcut("mod+shift+n")
                .on_select(open_new_project),
            MenuItem::new("Open Project…")
                .shortcut("mod+o")
                .on_select(open_project_dialog),
            recent_menu,
            MenuItem::separator(),
            MenuItem::new("Import Workflow…")
                .disabled(!snapshot.has_projects)
                .on_select(open_import_workflow),
            MenuItem::new("Export Workflow…")
                .shortcut("mod+shift+e")
                .disabled(!snapshot.on_workflows)
                .on_select(store::export_selected_overview_workflow),
            MenuItem::separator(),
            // Use `on_select` instead of `MenuRole::Close` so `MenuItem::window_menu`
            // binds AppKit's `windowsMenu` to the `Window` submenu rather than `File`.
            MenuItem::new("Close Window")
                .shortcut("mod+w")
                .on_select(close_active_window),
        ],
    )
}

fn edit_menu() -> MenuItem {
    MenuItem::submenu(
        "Edit",
        [
            MenuItem::role(MenuRole::Undo),
            MenuItem::role(MenuRole::Redo),
            MenuItem::separator(),
            MenuItem::role(MenuRole::Cut),
            MenuItem::role(MenuRole::Copy),
            MenuItem::role(MenuRole::Paste),
            MenuItem::role(MenuRole::SelectAll),
            MenuItem::separator(),
            MenuItem::new("Find…").shortcut("mod+f").disabled(true),
            MenuItem::new("Find in Run Output")
                .shortcut("alt+mod+f")
                .disabled(true),
        ],
    )
}

fn view_menu(snapshot: &MenuSnapshot) -> MenuItem {
    let sidebar_label = if snapshot.sidebar_visible {
        "Hide Sidebar"
    } else {
        "Show Sidebar"
    };
    let full_screen_label = if snapshot.is_full_screen {
        "Exit Full Screen"
    } else {
        "Enter Full Screen"
    };
    let is_full_screen = snapshot.is_full_screen;

    let has_inspector = snapshot.on_work_queue
        || snapshot.on_workflows
        || snapshot.on_workflow_editor
        || snapshot.on_workflow_run;
    let inspector_label = if has_inspector && snapshot.inspector_visible {
        "Hide Inspector"
    } else {
        "Show Inspector"
    };

    MenuItem::submenu(
        "View",
        [
            MenuItem::new(sidebar_label)
                .shortcut("ctrl+cmd+s")
                .on_select(store::toggle_sidebar),
            MenuItem::new(inspector_label)
                .shortcut("alt+mod+i")
                .disabled(!has_inspector)
                .on_select(toggle_active_inspector),
            MenuItem::new("Command Palette")
                .shortcut("mod+k")
                .disabled(!snapshot.on_workflow_run)
                .on_select(store::toggle_workflow_run_command_palette),
            MenuItem::separator(),
            MenuItem::new("Output")
                .shortcut("mod+1")
                .disabled(!snapshot.on_workflow_run)
                .on_select(|| store::set_workflow_run_tab(RunDetailTab::Output)),
            MenuItem::new("Diff")
                .shortcut("mod+2")
                .disabled(!snapshot.on_workflow_run)
                .on_select(|| store::set_workflow_run_tab(RunDetailTab::Diff)),
            MenuItem::new("Logs").shortcut("mod+3").disabled(true),
            MenuItem::separator(),
            MenuItem::new("Wrap Transcript Lines").disabled(true),
            // Use `on_select` instead of `MenuRole::ToggleFullScreen` so AppKit's
            // `windowsMenu` stays attached to the `Window` submenu.
            MenuItem::new(full_screen_label)
                .shortcut("ctrl+cmd+f")
                .on_select(move || toggle_full_screen(is_full_screen)),
            MenuItem::separator(),
            from_extensions_header(),
            MenuItem::new("Coverage")
                .shortcut("mod+4")
                .disabled(!snapshot.on_workflow_run)
                .on_select(|| store::set_workflow_run_tab(RunDetailTab::Coverage)),
        ],
    )
}

fn project_menu(snapshot: &MenuSnapshot) -> MenuItem {
    MenuItem::submenu(
        "Project",
        [
            MenuItem::new("Work Queue")
                .shortcut("mod+0")
                .disabled(!snapshot.has_projects)
                .on_select(open_work_queue_window),
            MenuItem::new("Sync Issues")
                .shortcut("mod+shift+i")
                .disabled(!snapshot.has_projects)
                .on_select(sync_project_issues),
            MenuItem::new("Issue Source & Intake…")
                .disabled(!snapshot.has_projects)
                .on_select(|| {
                    store::open_project_settings_tab(ProjectSettingsTab::IssuesAndIntake)
                }),
            MenuItem::separator(),
            MenuItem::new("Project Settings…")
                .disabled(!snapshot.has_projects)
                .on_select(store::open_project_settings),
            MenuItem::new("Environment Variables…")
                .disabled(!snapshot.has_projects)
                .on_select(|| store::open_project_settings_tab(ProjectSettingsTab::Environment)),
            MenuItem::new("Show in Finder")
                .shortcut("alt+mod+o")
                .disabled(!snapshot.has_selected_project)
                .on_select(reveal_selected_project_in_finder),
            MenuItem::separator(),
            MenuItem::new("Archive Project…").disabled(true),
        ],
    )
}

fn workflow_menu(snapshot: &MenuSnapshot) -> MenuItem {
    MenuItem::submenu(
        "Workflow",
        [
            MenuItem::new("Edit Workflow")
                .shortcut("mod+e")
                .disabled(!(snapshot.on_workflow_run || snapshot.on_workflows))
                .on_select(edit_active_workflow),
            MenuItem::new("Add Step…")
                .shortcut("alt+mod+n")
                .disabled(!snapshot.on_workflow_editor)
                .on_select(|| store::add_workflow_editor_step(StepTypeKind::ClaudeTask)),
            MenuItem::new("Validate")
                .shortcut("mod+shift+v")
                .disabled(!snapshot.on_workflow_editor)
                .on_select(store::validate_workflow_editor),
            MenuItem::separator(),
            MenuItem::new("Duplicate")
                .disabled(!snapshot.on_workflows)
                .on_select(store::duplicate_selected_overview_workflow),
            MenuItem::new("Version History…")
                .disabled(!snapshot.on_workflow_editor)
                .on_select(store::toggle_workflow_editor_history),
            MenuItem::separator(),
            from_extensions_header(),
            MenuItem::new("Post Update to Chat").disabled(true),
        ],
    )
}

fn run_menu(snapshot: &MenuSnapshot) -> MenuItem {
    let pause_label = if snapshot.workflow_run_paused {
        "Resume"
    } else {
        "Pause"
    };
    MenuItem::submenu(
        "Run",
        [
            MenuItem::new("Ad-hoc Run")
                .shortcut("mod+r")
                .disabled(!snapshot.has_projects)
                .on_select(open_ad_hoc_run),
            MenuItem::new("Ad-hoc Run with Inputs…")
                .shortcut("mod+shift+r")
                .disabled(true),
            MenuItem::new("Start Next Queued Issue")
                .shortcut("alt+mod+enter")
                .disabled(!snapshot.has_queued_issues)
                .on_select(store::start_next_queued_issue),
            MenuItem::separator(),
            MenuItem::new(pause_label)
                .shortcut("mod+.")
                .disabled(!snapshot.can_pause_run)
                .on_select(store::toggle_pause_workflow_run),
            MenuItem::new("Stop")
                .shortcut("mod+shift+.")
                .disabled(!snapshot.can_stop_run)
                .on_select(stop_active_run),
            MenuItem::new("Retry Failed Step")
                .shortcut("alt+mod+r")
                .disabled(true),
            MenuItem::separator(),
            MenuItem::new("Approve Step")
                .shortcut("mod+enter")
                .disabled(!snapshot.can_approve_step)
                .on_select(approve_active_step),
            MenuItem::new("Request Changes…").disabled(true),
            MenuItem::separator(),
            MenuItem::new("Run History")
                .shortcut("mod+y")
                .disabled(!snapshot.has_projects)
                .on_select(open_run_history),
            MenuItem::separator(),
            from_extensions_header(),
            MenuItem::new("Set Run Budget…").disabled(true),
        ],
    )
}

fn window_menu() -> MenuItem {
    MenuItem::submenu(
        "Window",
        [
            MenuItem::role(MenuRole::Minimize),
            MenuItem::role(MenuRole::Zoom),
            MenuItem::separator(),
            MenuItem::new("Dashboard")
                .shortcut("mod+shift+d")
                .on_select(open_dashboard_window),
            MenuItem::new("Inbox")
                .shortcut("mod+i")
                .on_select(open_inbox_window),
            MenuItem::new("Machines")
                .shortcut("alt+mod+m")
                .on_select(open_machines_window),
            MenuItem::new("Run Monitor")
                .shortcut("mod+shift+m")
                .disabled(true),
            MenuItem::separator(),
            MenuItem::new("Bring All to Front").disabled(true),
        ],
    )
}

fn help_menu() -> MenuItem {
    // macOS AppKit automatically inserts the native Help Search field at the top of
    // a top-level menu titled "Help".
    MenuItem::submenu(
        "Help",
        [
            MenuItem::new("Bruk Help").disabled(true),
            MenuItem::new("Workflow Reference").disabled(true),
            MenuItem::new("Extension API").disabled(true),
            MenuItem::separator(),
            MenuItem::new("Report an Issue…").disabled(true),
        ],
    )
}

fn from_extensions_header() -> MenuItem {
    MenuItem::new("From extensions")
        .icon(EXTENSIONS_ICON_URI)
        .disabled(true)
}

fn update_menu_item(state: &crate::updater::UpdaterState) -> MenuItem {
    use crate::updater::UpdaterState;
    match state {
        UpdaterState::Checking => MenuItem::new("Checking for Updates…").disabled(true),
        UpdaterState::Downloading { .. } => {
            let label = match state.download_percent() {
                Some(pct) => format!("Downloading Update ({pct}%)…"),
                None => "Downloading Update…".to_owned(),
            };
            MenuItem::new(label).disabled(true)
        }
        UpdaterState::Installing { .. } => MenuItem::new("Installing Update…").disabled(true),
        UpdaterState::ReadyToRestart { version, .. } => {
            MenuItem::new(format!("Restart to Update (v{version})…"))
                .on_select(store::install_and_restart_update)
        }
        UpdaterState::Available(update) if update.can_install => {
            MenuItem::new(format!("Download Update (v{})…", update.version)).on_select(|| {
                store::download_available_update();
                main_window().focus();
            })
        }
        UpdaterState::Available(update) => {
            MenuItem::new(format!("Download Update (v{})…", update.version))
                .on_select(store::open_update_download_url)
        }
        UpdaterState::StagedOnQuit { version } => {
            MenuItem::new(format!("Update Ready on Quit (v{version})")).disabled(true)
        }
        UpdaterState::Idle
        | UpdaterState::UpToDate { .. }
        | UpdaterState::Unavailable(_)
        | UpdaterState::Error(_) => MenuItem::new("Check for Updates…").on_select(|| {
            store::open_preferences_tab(PreferencesTab::General);
            store::check_for_updates();
        }),
    }
}

// --- Menu Action Handlers ---------------------------------------------------------------

fn open_new_project() {
    store::open_register_project();
    main_window().focus();
}

fn open_project_dialog() {
    let mut dialog = OpenDialog::folders()
        .title("Open Project Folder")
        .button_label("Open Project");
    if let Some(dir) = store::default_projects_dir_path() {
        dialog = dialog.directory(dir);
    }
    let answer = pick_paths(dialog);
    spawn(async move {
        if let Ok(Some(paths)) = answer.await
            && let Some(chosen) = paths.into_iter().next()
        {
            store::open_or_register_project_path(&chosen);
            main_window().focus();
        }
    });
}

fn close_active_window() {
    if store::preferences_open() && store::preferences_focused() {
        store::close_preferences();
    } else {
        main_window().close();
    }
}

fn toggle_full_screen(is_full_screen: bool) {
    let next_mode = if is_full_screen {
        WindowMode::Normal
    } else {
        WindowMode::FullScreen
    };
    main_window().set_mode(next_mode);
}

fn reveal_selected_project_in_finder() {
    let Some(project) = store::selected_project() else {
        return;
    };
    let path = project.root_path;
    let answer = reveal_path(path.clone());
    spawn(async move {
        if let Err(err) = answer.await {
            store::set_status_message(format!(
                "Could not show '{}' in Finder: {err}",
                path.display()
            ));
        }
    });
}

fn open_run_history() {
    store::open_workspace();
    main_window().focus();
}

fn open_dashboard_window() {
    store::open_dashboard();
    main_window().focus();
}

fn open_inbox_window() {
    store::open_inbox();
}

fn open_machines_window() {
    store::open_machines();
    main_window().focus();
}

fn open_work_queue_window() {
    store::open_work_queue();
    main_window().focus();
}

fn sync_project_issues() {
    store::open_work_queue();
    store::sync_work_queue();
    main_window().focus();
}

fn open_ad_hoc_run() {
    if store::active_view() == store::ActiveView::WorkflowEditor {
        store::set_workflow_editor_ad_hoc_open(true);
    } else {
        store::open_work_queue_ad_hoc();
    }
    main_window().focus();
}

fn open_new_workflow() {
    store::open_new_workflow_drawer();
    main_window().focus();
}

fn open_new_chat() {
    store::start_new_chat();
    main_window().focus();
}

fn open_import_workflow() {
    store::open_import_workflow_drawer();
    main_window().focus();
}

fn edit_active_workflow() {
    if store::active_view() == store::ActiveView::Workflows {
        store::open_workflow_editor_for_selected_overview();
    } else {
        store::edit_active_workflow();
    }
}

fn toggle_active_inspector() {
    match store::active_view() {
        store::ActiveView::WorkflowRun => store::toggle_workflow_run_inspector(),
        store::ActiveView::WorkflowEditor => store::toggle_workflow_editor_inspector(),
        store::ActiveView::Workflows => store::toggle_workflows_inspector(),
        store::ActiveView::WorkQueue => store::toggle_work_queue_inspector(),
        _ => {}
    }
}

fn stop_active_run() {
    if store::active_view() == store::ActiveView::WorkflowRun {
        store::stop_workflow_run();
    } else {
        store::cancel_selected_run();
    }
}

fn approve_active_step() {
    if store::active_view() == store::ActiveView::WorkflowRun {
        store::approve_workflow_run_review_gate();
    } else {
        store::approve_selected_run_step();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> MenuSnapshot {
        MenuSnapshot {
            demo_mode: false,
            sidebar_visible: true,
            is_full_screen: false,
            recent_projects: vec![(1, "api-gateway".to_owned()), (2, "mobile-app".to_owned())],
            has_selected_project: true,
            on_work_queue: true,
            on_workflows: false,
            on_workflow_editor: false,
            on_workflow_run: false,
            inspector_visible: true,
            has_queued_issues: true,
            can_pause_run: false,
            workflow_run_paused: false,
            can_stop_run: false,
            can_approve_step: false,
            has_projects: true,
            updater_state: crate::updater::UpdaterState::Idle,
        }
    }

    #[test]
    fn builds_all_nine_top_level_menus_with_valid_shortcuts() {
        let menu = build_application_menu(&sample_snapshot());
        assert_eq!(menu.len(), 9);
        let debug = format!("{menu:?}");
        for expected in [
            "Bruk",
            "Demo Mode",
            "File",
            "Edit",
            "View",
            "Project",
            "Workflow",
            "Run",
            "Window",
            "Help",
        ] {
            assert!(
                debug.contains(expected),
                "expected top-level menu '{expected}' in {debug}"
            );
        }
        assert!(
            !debug.contains("Software Factory"),
            "did not expect legacy 'Software Factory' label in {debug}"
        );
    }

    #[test]
    fn demo_mode_menu_item_appears_under_bruk_menu() {
        let mut snap = sample_snapshot();
        snap.demo_mode = false;
        let off = format!("{:?}", app_menu(&snap));
        assert!(off.contains("Bruk"));
        assert!(off.contains("Demo Mode"));
        assert!(!off.contains("Software Factory"));

        snap.demo_mode = true;
        let on = format!("{:?}", app_menu(&snap));
        assert!(on.contains("Bruk"));
        assert!(on.contains("Demo Mode"));
    }

    #[test]
    fn sidebar_and_fullscreen_labels_reflect_snapshot_state() {
        let mut snap = sample_snapshot();
        let initial = format!("{:?}", view_menu(&snap));
        assert!(initial.contains("Hide Sidebar"));
        assert!(initial.contains("Enter Full Screen"));

        snap.sidebar_visible = false;
        snap.is_full_screen = true;
        let toggled = format!("{:?}", view_menu(&snap));
        assert!(toggled.contains("Show Sidebar"));
        assert!(toggled.contains("Exit Full Screen"));
    }

    #[test]
    fn recent_projects_populate_open_recent_submenu() {
        let snap = sample_snapshot();
        let file = format!("{:?}", file_menu(&snap));
        assert!(file.contains("Open Recent"));
        assert!(file.contains("api-gateway"));
        assert!(file.contains("mobile-app"));
    }

    #[test]
    fn app_menu_update_item_reflects_updater_state() {
        let mut snap = sample_snapshot();
        assert!(format!("{:?}", app_menu(&snap)).contains("Check for Updates…"));

        snap.updater_state = crate::updater::UpdaterState::Available(crate::updater::AvailableUpdate {
            version: "0.2.0".to_owned(),
            notes: None,
            url: "https://github.com/frithuz/Bruk/releases/latest/download/Bruk-0.2.0-macos-aarch64.tar.gz".to_owned(),
            sha256: Some("a".repeat(64)),
            size: Some(1024),
            can_install: true,
            install_blocked_reason: None,
        });
        assert!(format!("{:?}", app_menu(&snap)).contains("Download Update (v0.2.0)…"));

        snap.updater_state = crate::updater::UpdaterState::ReadyToRestart {
            version: "0.2.0".to_owned(),
            notes: None,
            archive_path: std::path::PathBuf::from("/tmp/Bruk-0.2.0.tar.gz"),
        };
        assert!(format!("{:?}", app_menu(&snap)).contains("Restart to Update (v0.2.0)…"));
    }
}
