//! Software Factory macOS desktop UI built with Native Shell (`nsh`).

pub mod boot_log;
mod fonts;
mod menu;
mod store;
pub mod system_log;
mod ui;

use std::{ffi::OsString, path::PathBuf};

pub use fonts::{EMBEDDED_FONTS, default_tokens, tokens_for_appearance, with_embedded_fonts};
use nsh_base::theme;
use nsh_core::{element::Root, prelude::*};

/// Registers this crate's Native Shell UI set (`nsh.json` `"rust"`).
#[native_module("software-factory/native")]
fn native(registry: &mut Registry) {
    boot_log::init("nsh_native_module");
    let _span = boot_log::span("boot.native_module.register");
    registry.register::<App>().assets(assets!());
}

/// Root component mounted by `cargo run`, `nsh run`, and `nsh package`.
///
/// Opens the Bruk folder (`~/Bruk` by default) and loads tracked projects, or shows
/// first-launch setup when no folder has been chosen yet.
#[component]
pub fn App() -> impl IntoElement {
    boot_log::init("app_component");
    let mount_span = boot_log::span("boot.app.mount");
    if let Some(age_ms) = boot_log::process_age_ms() {
        boot_log::event("boot.app.mount.enter", &format!("proc_age_ms={age_ms:.1}"));
    }
    store::init_default();
    let surface = app_surface();
    boot_log::schedule_main_thread_ready();
    let summary = format!(
        "needs_home_setup={} active_view={:?}",
        store::needs_home_setup(),
        store::active_view()
    );
    mount_span.finish(&summary);
    boot_log::finish_sync_boot(&summary);
    surface
}

/// Builds a Native Shell [`Root`] backed by a specific SQLite database file.
///
/// Useful for isolated tests or custom storage locations.
pub fn root_with_db_path(db_path: PathBuf) -> Root {
    let db_path = db_path.to_string_lossy().into_owned();
    Root::new::<AppWithDb>(move || AppWithDbProps {
        db_path: db_path.clone(),
        open_menubar_popover: false,
        claude_path_env: None,
    })
}

/// Builds a Native Shell [`Root`] backed by a specific SQLite database file and an explicit
/// `PATH` search value for deterministic Claude CLI discovery in tests.
pub fn root_with_db_path_and_claude_path(db_path: PathBuf, path_env: OsString) -> Root {
    let db_path = db_path.to_string_lossy().into_owned();
    let claude_path_env = Some(path_env.to_string_lossy().into_owned());
    Root::new::<AppWithDb>(move || AppWithDbProps {
        db_path: db_path.clone(),
        open_menubar_popover: false,
        claude_path_env: claude_path_env.clone(),
    })
}

/// Builds a Native Shell [`Root`] backed by a specific SQLite database file
/// with the menu bar popover opened on launch (for headless UI tests).
pub fn root_with_db_path_and_menubar_popover(db_path: PathBuf) -> Root {
    let db_path = db_path.to_string_lossy().into_owned();
    Root::new::<AppWithDb>(move || AppWithDbProps {
        db_path: db_path.clone(),
        open_menubar_popover: true,
        claude_path_env: Some(String::new()),
    })
}

/// Builds a Native Shell [`Root`] that resolves the Bruk folder from an explicit
/// [`factory_core::HomeEnvironment`] instead of this machine's, so first-launch setup can be
/// exercised in isolated tests. `path_env` is the `PATH` used for provider CLI discovery.
pub fn root_with_home_environment(
    environment: factory_core::HomeEnvironment,
    path_env: OsString,
) -> Root {
    let props = AppWithHomeProps {
        env_override: environment.env_override.map(path_string),
        default_root: path_string(environment.default_root),
        locator_path: path_string(environment.locator_path),
        legacy_database_paths: environment
            .legacy_database_paths
            .into_iter()
            .map(path_string)
            .collect(),
        claude_path_env: path_env.to_string_lossy().into_owned(),
    };
    Root::new::<AppWithHome>(move || props.clone())
}

fn path_string(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Clone, PartialEq, Props)]
struct AppWithHomeProps {
    env_override: Option<String>,
    default_root: String,
    locator_path: String,
    legacy_database_paths: Vec<String>,
    claude_path_env: String,
}

#[component]
fn AppWithHome(props: AppWithHomeProps) -> impl IntoElement {
    store::init_with_home_environment(
        factory_core::HomeEnvironment {
            env_override: props.env_override.as_ref().map(PathBuf::from),
            default_root: PathBuf::from(&props.default_root),
            locator_path: PathBuf::from(&props.locator_path),
            legacy_database_paths: props
                .legacy_database_paths
                .iter()
                .map(PathBuf::from)
                .collect(),
        },
        store::ClaudePathLookup::ExplicitPath(OsString::from(&props.claude_path_env)),
    );
    app_surface()
}

#[derive(Clone, PartialEq, Props)]
struct AppWithDbProps {
    db_path: String,
    open_menubar_popover: bool,
    claude_path_env: Option<String>,
}

#[component]
fn AppWithDb(props: AppWithDbProps) -> impl IntoElement {
    match &props.claude_path_env {
        Some(path_env) => store::init_with_database_path_and_claude_path(
            std::path::Path::new(&props.db_path),
            store::ClaudePathLookup::ExplicitPath(OsString::from(path_env)),
        ),
        None => store::init_with_database_path(std::path::Path::new(&props.db_path)),
    }
    if props.open_menubar_popover {
        store::open_menubar_popover(None);
    }
    app_surface()
}

fn app_surface() -> impl IntoElement {
    {
        let _span = boot_log::span("boot.ui.window_attrs");
        let mut title_bar = nsh_core::desktop::TitleBarOptions::default();
        title_bar.style = nsh_core::desktop::TitleBarStyle::Hidden;
        title_bar.height = Some(px(54.0));
        let mut attrs = nsh_core::desktop::WindowAttributes::default();
        attrs.title_bar = Some(title_bar);
        nsh_core::element::desktop::current_window().set(attrs);
    }

    {
        let _span = boot_log::span("boot.ui.install_menu");
        menu::install_application_menu();
    }
    {
        let _span = boot_log::span("boot.ui.install_menubar");
        ui::menubar::install_menubar_and_attention_surfaces();
    }

    let _span = boot_log::span("boot.ui.build_root_view");
    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(when(
            store::needs_home_setup,
            ui::home_setup::home_setup_view,
        ))
        .child(when(|| !store::needs_home_setup(), ui::shell))
        .child(ui::menubar::menubar_popover_window())
        .child(ui::preferences::preferences_window())
}
