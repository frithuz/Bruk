//! First-launch setup: choose where the Bruk folder lives.

use std::path::Path;

use nsh_base::{button, input, theme};
use nsh_core::{file_dialog::OpenDialog, prelude::*};

use factory_store as store;
use factory_ui_kit::palette;

/// Full-window first-launch screen shown until a Bruk folder is chosen.
pub fn home_setup_view() -> impl IntoElement {
    static FIRST_BUILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let _boot_span =
        factory_logging::boot_log::once_span("boot.ui.home_setup_first_build", &FIRST_BUILD);
    view()
        .flex_1()
        .bg(palette::canvas_bg())
        .child(view().h(px(54.0)).flex_shrink_0().window_drag(true))
        .child(
            scroll().flex_1().child(
                view()
                    .flex_1()
                    .items_center()
                    .px(px(24.0))
                    .py(px(32.0))
                    .child(setup_card()),
            ),
        )
}

fn setup_card() -> impl IntoElement {
    view()
        .w(px(580.0))
        .p(px(26.0))
        .rounded(px(14.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(18.0))
        .child(
            view()
                .gap(px(6.0))
                .child(
                    text("Welcome to Bruk")
                        .text_size(px(20.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("Choose where Bruk keeps your data. Your preferences, database, and the projects Bruk clones all live in this one folder, so it's easy to find, back up, or inspect.")
                        .text_size(px(13.0))
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .gap(px(6.0))
                .child(
                    text("Bruk folder")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            input()
                                .value(store::home_setup_root_input)
                                .on_change(store::set_home_setup_root_input)
                                .on_submit(|_: &str| store::complete_home_setup())
                                .placeholder("~/Bruk")
                                .test_id("home-setup-root")
                                .flex_1()
                                .px(px(11.0))
                                .py(px(8.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_inset_bg())
                                .rounded(px(8.0))
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            button()
                                .on_press(choose_folder)
                                .px(px(12.0))
                                .py(px(8.0))
                                .rounded(px(8.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.75))
                                .child(
                                    text("Choose…")
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                )
                .child(
                    text(move || {
                        if store::home_setup_target_has_database() {
                            "This folder already has a Bruk database. Bruk will use it as it is."
                                .to_owned()
                        } else {
                            "Bruk creates bruk.db, projects/, runs/, and cache/ here. Repositories you already have can stay where they are and be linked later."
                                .to_owned()
                        }
                    })
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                ),
        )
        .child(when(
            || store::home_setup_legacy_database().is_some(),
            legacy_import_row,
        ))
        .child(when(
            || store::home_setup_error().is_some(),
            || {
                text(move || store::home_setup_error().unwrap_or_default())
                    .text_size(px(12.0))
                    .text_color(move || theme().destructive)
            },
        ))
        .child(
            view().flex_row().justify_end().child(
                button()
                    .test_id("home-setup-continue")
                    .disabled(|| store::home_setup_root_input().trim().is_empty())
                    .on_press(store::complete_home_setup)
                    .px(px(16.0))
                    .py(px(8.5))
                    .rounded(px(8.0))
                    .bg(palette::nav_active_bg())
                    .active(|s| s.opacity(0.8))
                    .disabled_style(|s| s.opacity(0.4))
                    .child(
                        text(move || {
                            if store::home_setup_target_has_database() {
                                "Use This Folder".to_owned()
                            } else {
                                "Create Bruk Folder".to_owned()
                            }
                        })
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                    ),
            ),
        )
}

fn legacy_import_row() -> impl IntoElement {
    button()
        .test_id("home-setup-import-legacy")
        .accessibility_label("Import existing projects and preferences")
        .on_press(store::toggle_home_setup_import_legacy)
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .p(px(12.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .child(
            view()
                .w(px(16.0))
                .h(px(16.0))
                .flex_shrink_0()
                .rounded(px(4.0))
                .border_1()
                .border_color(palette::border_color())
                .items_center()
                .justify_center()
                .bg(move || {
                    if store::home_setup_import_legacy() {
                        palette::nav_active_bg()
                    } else {
                        palette::control_bg()
                    }
                })
                .child(when(store::home_setup_import_legacy, || {
                    text("✓")
                        .text_size(px(11.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(palette::nav_active_fg())
                })),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text("Import existing projects and preferences")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(move || {
                        format!(
                            "Copies {} into the new folder. The original is left in place.",
                            store::home_setup_legacy_database().unwrap_or_default()
                        )
                    })
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                ),
        )
}

fn choose_folder() {
    let answer = pick_paths(
        OpenDialog::folders()
            .title("Choose Bruk Folder")
            .button_label("Use Folder"),
    );
    spawn(async move {
        if let Ok(Some(paths)) = answer.await
            && let Some(chosen) = paths.into_iter().next()
        {
            store::set_home_setup_root_input(&factory_config::display_with_tilde(
                &bruk_folder_for(&chosen),
            ));
        }
    });
}

/// Picking a non-empty folder that isn't already a Bruk folder (say `~/Code`) proposes a
/// `Bruk` subfolder inside it, so Bruk's files don't mix with what's already there.
fn bruk_folder_for(chosen: &Path) -> std::path::PathBuf {
    let is_bruk_folder = factory_config::BrukHome::at(chosen).is_initialized();
    let is_empty = std::fs::read_dir(chosen).is_ok_and(|mut entries| entries.next().is_none());
    if is_bruk_folder || is_empty {
        chosen.to_path_buf()
    } else {
        chosen.join(factory_config::DEFAULT_HOME_DIR_NAME)
    }
}
