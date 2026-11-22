//! Dedicated Project Registration view backed by the local SQLite database.

use factory_core::ProjectId;
use nsh_base::{button, input, theme};
use nsh_core::{file_dialog::OpenDialog, prelude::*};

use crate::{store, ui::palette};

/// Full-canvas view for registering repositories and managing tracked projects.
pub(crate) fn register_project_view() -> impl IntoElement {
    let project_name = signal(String::new());
    let repo_path = signal(String::new());

    let submit = move || {
        let name = project_name.get_untracked();
        let path = repo_path.get_untracked();
        if store::register_project(&name, &path) {
            project_name.set(String::new());
            repo_path.set(String::new());
        }
    };

    let browse_folder = move || {
        let answer = pick_paths(
            OpenDialog::folders()
                .title("Select Repository Folder")
                .button_label("Select Repository"),
        );
        spawn(async move {
            if let Ok(Some(paths)) = answer.await
                && let Some(chosen) = paths.into_iter().next()
            {
                if project_name.with_untracked(|name| name.trim().is_empty())
                    && let Some(folder_name) = chosen.file_name().and_then(|n| n.to_str())
                {
                    project_name.set(folder_name.to_owned());
                }
                repo_path.set(chosen.display().to_string());
            }
        });
    };

    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(18.0))
            .gap(px(16.0))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        view()
                            .gap(px(2.0))
                            .child(
                                text("Register a local repository")
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::text_primary()),
                            )
                            .child(
                                text("Track local repositories in SQLite so AI workflows can run against them.")
                                    .text_size(px(12.5))
                                    .text_color(palette::text_muted()),
                            ),
                    )
                    .child(when(
                        || !store::tracked_project_ids().is_empty(),
                        || {
                            button()
                                .on_press(store::open_workspace)
                                .px(px(12.0))
                                .py(px(6.5))
                                .rounded(px(8.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.75))
                                .child(
                                    text("Open Workspace")
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                )
                        },
                    )),
            )
            .child(
                view()
                    .p(px(18.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_bg())
                    .gap(px(14.0))
                    .child(
                        view()
                            .gap(px(6.0))
                            .child(
                                text("Project Name")
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_primary()),
                            )
                            .child(
                                input()
                                    .value(move || project_name.get())
                                    .on_change(move |value: &str| {
                                        project_name.set(value.to_owned())
                                    })
                                    .placeholder("Project name")
                                    .px(px(11.0))
                                    .py(px(8.0))
                                    .border_1()
                                    .border_color(palette::border_color())
                                    .bg(palette::card_inset_bg())
                                    .rounded(px(8.0))
                                    .text_color(palette::text_primary()),
                            ),
                    )
                    .child(
                        view()
                            .gap(px(6.0))
                            .child(
                                text("Repository Path")
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
                                            .value(move || repo_path.get())
                                            .on_change(move |value: &str| {
                                                repo_path.set(value.to_owned())
                                            })
                                            .on_submit(move |_: &str| submit())
                                            .placeholder("Repository path")
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
                                            .on_press(browse_folder)
                                            .px(px(12.0))
                                            .py(px(8.0))
                                            .rounded(px(8.0))
                                            .border_1()
                                            .border_color(palette::border_color())
                                            .bg(palette::control_bg())
                                            .hover(|s| s.bg(palette::subtle_surface_bg()))
                                            .active(|s| s.opacity(0.75))
                                            .child(
                                                text("Browse...")
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(palette::text_primary()),
                                            ),
                                    ),
                            ),
                    )
                    .child(when(
                        || store::registration_error().is_some(),
                        || {
                            text(move || store::registration_error().unwrap_or_default())
                                .text_size(px(12.0))
                                .text_color(move || theme().destructive)
                        },
                    ))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .child(
                                text(move || format!("SQLite DB: {}", store::db_path_display()))
                                    .text_size(px(11.5))
                                    .text_color(palette::text_muted()),
                            )
                            .child(
                                button()
                                    .disabled(move || {
                                        project_name.with(|n| n.trim().is_empty())
                                            || repo_path.with(|p| p.trim().is_empty())
                                    })
                                    .on_press(submit)
                                    .px(px(14.0))
                                    .py(px(8.0))
                                    .rounded(px(8.0))
                                    .bg(palette::nav_active_bg())
                                    .active(|s| s.opacity(0.8))
                                    .disabled_style(|s| s.opacity(0.4))
                                    .child(
                                        text("Save Project")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::nav_active_fg()),
                                    ),
                            ),
                    ),
            )
            .child(
                view()
                    .p(px(16.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_bg())
                    .gap(px(10.0))
                    .child(
                        text("Tracked projects")
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                    )
                    .child(when(
                        || store::tracked_project_ids().is_empty(),
                        || {
                            text("No projects registered in SQLite yet. Add a repository name and path above to save it locally.")
                                .text_size(px(12.5))
                                .text_color(palette::text_muted())
                        },
                    ))
                    .child(each(
                        store::tracked_project_ids,
                        |id: &ProjectId| *id,
                        |id, _| tracked_project_row(id.get_untracked()),
                    )),
            ),
    )
}

fn tracked_project_row(id: ProjectId) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .p(px(12.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .gap(px(12.0))
        .child(
            view()
                .flex_1()
                .gap(px(2.0))
                .child(
                    text(move || store::project_by_id(id).map(|p| p.name).unwrap_or_default())
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(move || {
                        store::project_by_id(id)
                            .map(|p| p.root_path.display().to_string())
                            .unwrap_or_default()
                    })
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    button()
                        .on_press(move || store::select_project(id))
                        .px(px(10.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .active(|s| s.opacity(0.75))
                        .child(
                            text("Open")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    button()
                        .on_press(move || store::remove_project(id))
                        .px(px(10.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(move || theme().destructive)
                        .active(|s| s.opacity(0.75))
                        .child(
                            text("Remove")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(move || theme().destructive),
                        ),
                ),
        )
}
