//! Software Factory macOS desktop UI built with Native Shell (`nsh`).

mod store;
mod ui;

use nsh_base::theme;
use nsh_core::prelude::*;

/// Registers this crate's Native Shell UI set (`nsh.json` `"rust"`).
#[native_module("software-factory/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// Root component mounted by `cargo run`, `nsh run`, and `nsh package`.
#[component]
pub fn App() -> impl IntoElement {
    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(ui::workspace())
}
