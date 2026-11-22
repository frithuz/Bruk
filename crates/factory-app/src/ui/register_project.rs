//! Dedicated Project Registration view backed by the local SQLite database.

use factory_core::ProjectId;
use nsh_base::{button, input, theme};
use nsh_core::{file_dialog::OpenDialog, prelude::*};

use crate::store;

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

    scroll().flex_1().child(
        view()
            .flex_1()
            .gap_6()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        view()
                            .gap_1()
                            .child(
                                text("Register Project")
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || theme().foreground),
                            )
                            .child(
                                text("Track local repositories in SQLite so AI workflows can run against them.")
                                    .text_sm()
                                    .text_color(move || theme().muted_foreground),
                            ),
                    )
                    .child(when(
                        || !store::project_ids().is_empty(),
                        || {
                            button()
                                .on_press(store::open_workspace)
                                .px_3()
                                .py_2()
                                .rounded(move || theme().radius)
                                .border_1()
                                .border_color(move || theme().border)
                                .active(|s| s.opacity(0.75))
                                .child(
                                    text("Open Workspace")
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || theme().foreground),
                                )
                        },
                    )),
            )
            .child(
                view()
                    .p_5()
                    .rounded(move || theme().radius)
                    .border_1()
                    .border_color(move || theme().border)
                    .bg(move || theme().card)
                    .gap_4()
                    .child(
                        view()
                            .gap_2()
                            .child(
                                text("Project Name")
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(move || theme().card_foreground),
                            )
                            .child(
                                input()
                                    .value(move || project_name.get())
                                    .on_change(move |value: &str| {
                                        project_name.set(value.to_owned())
                                    })
                                    .placeholder("Project name")
                                    .px_3()
                                    .py_2()
                                    .border_1()
                                    .border_color(move || theme().input)
                                    .rounded(move || theme().radius)
                                    .text_color(move || theme().foreground),
                            ),
                    )
                    .child(
                        view()
                            .gap_2()
                            .child(
                                text("Repository Path")
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(move || theme().card_foreground),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        input()
                                            .value(move || repo_path.get())
                                            .on_change(move |value: &str| {
                                                repo_path.set(value.to_owned())
                                            })
                                            .on_submit(move |_: &str| submit())
                                            .placeholder("Repository path")
                                            .flex_1()
                                            .px_3()
                                            .py_2()
                                            .border_1()
                                            .border_color(move || theme().input)
                                            .rounded(move || theme().radius)
                                            .text_color(move || theme().foreground),
                                    )
                                    .child(
                                        button()
                                            .on_press(browse_folder)
                                            .px_3()
                                            .py_2()
                                            .rounded(move || theme().radius)
                                            .bg(move || theme().secondary)
                                            .active(|s| s.opacity(0.75))
                                            .child(
                                                text("Browse...")
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(move || theme().secondary_foreground),
                                            ),
                                    ),
                            ),
                    )
                    .child(when(
                        || store::registration_error().is_some(),
                        || {
                            text(move || store::registration_error().unwrap_or_default())
                                .text_xs()
                                .text_color(move || theme().destructive)
                        },
                    ))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                text(move || format!("SQLite DB: {}", store::db_path_display()))
                                    .text_xs()
                                    .text_color(move || theme().muted_foreground),
                            )
                            .child(
                                button()
                                    .disabled(move || {
                                        project_name.with(|n| n.trim().is_empty())
                                            || repo_path.with(|p| p.trim().is_empty())
                                    })
                                    .on_press(submit)
                                    .px_4()
                                    .py_2()
                                    .rounded(move || theme().radius)
                                    .bg(move || theme().primary)
                                    .active(|s| s.opacity(0.8))
                                    .disabled_style(|s| s.opacity(0.4))
                                    .child(
                                        text("Save Project")
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(move || theme().primary_foreground),
                                    ),
                            ),
                    ),
            )
            .child(
                view()
                    .p_4()
                    .rounded(move || theme().radius)
                    .border_1()
                    .border_color(move || theme().border)
                    .bg(move || theme().card)
                    .gap_3()
                    .child(
                        text("TRACKED PROJECTS")
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(move || theme().muted_foreground),
                    )
                    .child(when(
                        || store::project_ids().is_empty(),
                        || {
                            text("No projects registered yet. Add a repository name and path above to save it locally.")
                                .text_sm()
                                .text_color(move || theme().muted_foreground)
                        },
                    ))
                    .child(each(
                        store::project_ids,
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
        .p_3()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().background)
        .gap_3()
        .child(
            view()
                .flex_1()
                .gap_1()
                .child(
                    text(move || store::project_by_id(id).map(|p| p.name).unwrap_or_default())
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme().foreground),
                )
                .child(
                    text(move || {
                        store::project_by_id(id)
                            .map(|p| p.root_path.display().to_string())
                            .unwrap_or_default()
                    })
                    .text_xs()
                    .text_color(move || theme().muted_foreground),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    button()
                        .on_press(move || store::select_project(id))
                        .px_3()
                        .py_1()
                        .rounded(move || theme().radius)
                        .bg(move || theme().secondary)
                        .active(|s| s.opacity(0.75))
                        .child(
                            text("Open")
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(move || theme().secondary_foreground),
                        ),
                )
                .child(
                    button()
                        .on_press(move || store::remove_project(id))
                        .px_3()
                        .py_1()
                        .rounded(move || theme().radius)
                        .border_1()
                        .border_color(move || theme().destructive)
                        .active(|s| s.opacity(0.75))
                        .child(
                            text("Remove")
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(move || theme().destructive),
                        ),
                ),
        )
}
