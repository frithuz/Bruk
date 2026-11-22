//! Software Factory macOS desktop UI built with Native Shell (`nsh`).

mod store;
mod ui;

use std::path::PathBuf;

use nsh_base::theme;
use nsh_core::{element::Root, prelude::*};

/// Registers this crate's Native Shell UI set (`nsh.json` `"rust"`).
#[native_module("software-factory/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// Root component mounted by `cargo run`, `nsh run`, and `nsh package`.
///
/// Opens the SQLite database in the user's config folder and loads tracked projects.
#[component]
pub fn App() -> impl IntoElement {
    store::init_default();
    app_surface()
}

/// Builds a Native Shell [`Root`] backed by a specific SQLite database file.
///
/// Useful for isolated tests or custom storage locations.
pub fn root_with_db_path(db_path: PathBuf) -> Root {
    let db_path = db_path.to_string_lossy().into_owned();
    Root::new::<AppWithDb>(move || AppWithDbProps {
        db_path: db_path.clone(),
    })
}

#[derive(Clone, PartialEq, Props)]
struct AppWithDbProps {
    db_path: String,
}

#[component]
fn AppWithDb(props: AppWithDbProps) -> impl IntoElement {
    store::init_with_database_path(std::path::Path::new(&props.db_path));
    app_surface()
}

fn app_surface() -> impl IntoElement {
    let mut title_bar = nsh_core::desktop::TitleBarOptions::default();
    title_bar.style = nsh_core::desktop::TitleBarStyle::Hidden;
    title_bar.height = Some(px(54.0));
    let mut attrs = nsh_core::desktop::WindowAttributes::default();
    attrs.title_bar = Some(title_bar);
    nsh_core::element::desktop::current_window().set(attrs);

    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(ui::shell())
}
