//! Project registration: clone a repository into the Bruk folder, or link an existing clone.

use factory_core::ProjectId;
use nsh_base::{button, input, theme};
use nsh_core::{file_dialog::OpenDialog, prelude::*};

use crate::{store, ui::palette};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegisterMode {
    /// Clone a remote into `<Bruk folder>/projects/<name>`.
    Clone,
    /// Track a clone that already exists elsewhere on disk.
    Link,
}

/// Full-canvas view for adding projects and managing tracked projects.
pub(crate) fn register_project_view() -> impl IntoElement {
    let mode = signal(RegisterMode::Clone);

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
                                text("Add a project")
                                    .text_size(px(16.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::text_primary()),
                            )
                            .child(
                                text("Clone a repository into your Bruk folder, or link a clone you already have.")
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
                    .flex_row()
                    .gap(px(2.0))
                    .p(px(2.0))
                    .self_start()
                    .rounded(px(9.0))
                    .bg(palette::segmented_track_bg())
                    .child(mode_button(
                        mode,
                        RegisterMode::Clone,
                        "register-mode-clone",
                        "Clone repository",
                    ))
                    .child(mode_button(
                        mode,
                        RegisterMode::Link,
                        "register-mode-link",
                        "Link existing clone",
                    )),
            )
            .child(when(move || mode.get() == RegisterMode::Clone, clone_card))
            .child(when(move || mode.get() == RegisterMode::Link, link_card))
            .child(tracked_projects_card()),
    )
}

fn mode_button(
    mode: Signal<RegisterMode>,
    value: RegisterMode,
    id: &'static str,
    label: &'static str,
) -> impl IntoElement {
    button()
        .test_id(id)
        .accessibility_label(label)
        .on_press(move || {
            mode.set(value);
            store::clear_registration_error();
        })
        .px(px(12.0))
        .py(px(5.5))
        .rounded(px(7.0))
        .bg(move || {
            if mode.get() == value {
                palette::control_bg()
            } else {
                palette::segmented_track_bg()
            }
        })
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if mode.get() == value {
                        palette::text_primary()
                    } else {
                        palette::text_secondary()
                    }
                }),
        )
}

fn form_card() -> View {
    view()
        .p(px(18.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(14.0))
}

fn field_label(label: &'static str) -> impl IntoElement {
    text(label)
        .text_size(px(12.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::text_primary())
}

fn registration_error_text() -> impl IntoElement {
    when(
        || store::registration_error().is_some(),
        || {
            text(move || store::registration_error().unwrap_or_default())
                .text_size(px(12.0))
                .text_color(move || theme().destructive)
        },
    )
}

fn primary_button_label(label: impl Fn() -> String + 'static) -> impl IntoElement {
    text(label)
        .text_size(px(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::nav_active_fg())
}

fn text_input() -> Input {
    input()
        .px(px(11.0))
        .py(px(8.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .rounded(px(8.0))
        .text_color(palette::text_primary())
}

fn clone_card() -> impl IntoElement {
    form_card()
        .child(
            view().gap(px(6.0)).child(field_label("Repository")).child(
                text_input()
                    .test_id("register-clone-source")
                    .value(store::clone_source_input)
                    .on_change(store::set_clone_source_input)
                    .on_submit(|_: &str| store::clone_and_register_project())
                    .placeholder("https://github.com/owner/repo.git or owner/repo"),
            ),
        )
        .child(
            view()
                .gap(px(6.0))
                .child(field_label("Project Name"))
                .child(
                    text_input()
                        .test_id("register-clone-name")
                        .value(store::clone_name_input)
                        .on_change(store::set_clone_name_input)
                        .placeholder("Project name"),
                )
                .child(
                    text(store::clone_destination_caption)
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(registration_error_text())
        .child(
            view().flex_row().items_center().justify_end().child(
                button()
                    .accessibility_label("Clone Repository")
                    .disabled(|| {
                        store::clone_in_progress() || store::clone_source_input().trim().is_empty()
                    })
                    .on_press(store::clone_and_register_project)
                    .px(px(14.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .bg(palette::nav_active_bg())
                    .active(|s| s.opacity(0.8))
                    .disabled_style(|s| s.opacity(0.4))
                    .child(primary_button_label(|| {
                        if store::clone_in_progress() {
                            "Cloning…".to_owned()
                        } else {
                            "Clone Repository".to_owned()
                        }
                    })),
            ),
        )
}

fn link_card() -> impl IntoElement {
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
        let mut dialog = OpenDialog::folders()
            .title("Select Repository Folder")
            .button_label("Select Repository");
        if let Some(dir) = store::default_projects_dir_path() {
            dialog = dialog.directory(dir);
        }
        let answer = pick_paths(dialog);
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

    form_card()
        .child(
            view()
                .gap(px(6.0))
                .child(field_label("Project Name"))
                .child(
                    text_input()
                        .value(move || project_name.get())
                        .on_change(move |value: &str| project_name.set(value.to_owned()))
                        .placeholder("Project name"),
                ),
        )
        .child(
            view()
                .gap(px(6.0))
                .child(field_label("Repository Path"))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text_input()
                                .value(move || repo_path.get())
                                .on_change(move |value: &str| repo_path.set(value.to_owned()))
                                .on_submit(move |_: &str| submit())
                                .placeholder("Repository path")
                                .flex_1(),
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
                )
                .child(
                    text("The clone stays where it is. Bruk only records its path.")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(registration_error_text())
        .child(
            view().flex_row().items_center().justify_end().child(
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
                    .child(primary_button_label(|| "Link Project".to_owned())),
            ),
        )
}

fn tracked_projects_card() -> impl IntoElement {
    view()
        .p(px(16.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    text("Tracked projects")
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(move || format!("Bruk folder: {}", store::home_root_display()))
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(when(
            || store::tracked_project_ids().is_empty(),
            || {
                text("No projects yet. Clone a repository or link an existing clone above.")
                    .text_size(px(12.5))
                    .text_color(palette::text_muted())
            },
        ))
        .child(each(
            store::tracked_project_ids,
            |id: &ProjectId| *id,
            |id, _| tracked_project_row(id.get_untracked()),
        ))
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
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(move || {
                                store::project_by_id(id).map(|p| p.name).unwrap_or_default()
                            })
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                        )
                        .child(
                            view()
                                .px(px(6.0))
                                .py(px(1.5))
                                .rounded(px(5.0))
                                .bg(palette::subtle_surface_bg())
                                .child(
                                    text(move || {
                                        if store::project_is_managed(id) {
                                            "In Bruk folder".to_owned()
                                        } else {
                                            "External clone".to_owned()
                                        }
                                    })
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_secondary()),
                                ),
                        ),
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
