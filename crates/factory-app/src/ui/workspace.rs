//! Project workflow workspace: workflow selector, task launcher, run history, and step inspector.

use factory_core::{RunId, RunStatus, StepRunStatus, WorkflowId};
use nsh_base::{button, input, theme};
use nsh_core::prelude::*;

use crate::{store, ui::palette};

pub(crate) fn workflow_workspace() -> impl IntoElement {
    let task_prompt = signal(String::new());
    let submit_run = move || {
        let prompt = task_prompt.get_untracked();
        if store::launch_run(&prompt) {
            task_prompt.set(String::new());
        }
    };

    view()
        .flex_1()
        .bg(palette::canvas_bg())
        .child(when(
            || store::selected_project().is_none(),
            || {
                view()
                    .test_id("workspace-empty-state")
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .p(px(32.0))
                    .gap(px(10.0))
                    .child(
                        text("No project selected")
                            .text_size(px(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                    )
                    .child(
                        text(
                            "Register a local repository to run workflows and inspect run history.",
                        )
                        .text_size(px(13.0))
                        .text_color(palette::text_muted()),
                    )
                    .child(
                        button()
                            .on_press(store::open_register_project)
                            .mt(px(4.0))
                            .h(px(34.0))
                            .px(px(15.0))
                            .rounded(px(8.0))
                            .bg(palette::nav_active_bg())
                            .items_center()
                            .justify_center()
                            .child(
                                text("Register a Project")
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::nav_active_fg()),
                            ),
                    )
            },
        ))
        .child(when(
            || store::selected_project().is_some(),
            move || {
                view()
                    .flex_1()
                    .p(px(18.0))
                    .gap(px(14.0))
                    .child(workflow_selector_card())
                    .child(launch_bar(task_prompt, submit_run))
                    .child(
                        view()
                            .flex_1()
                            .flex_row()
                            .gap(px(14.0))
                            .child(runs_list_panel())
                            .child(run_inspector_panel()),
                    )
            },
        ))
}

fn workflow_selector_card() -> impl IntoElement {
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
                    text(move || {
                        store::selected_project()
                            .map(|p| p.name)
                            .unwrap_or_else(|| "No project selected".to_owned())
                    })
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            text(move || {
                                store::selected_project()
                                    .map(|p| p.root_path.display().to_string())
                                    .unwrap_or_default()
                            })
                            .text_size(px(12.0))
                            .text_color(palette::text_muted()),
                        )
                        .child(
                            button()
                                .test_id("workspace-project-settings")
                                .accessibility_label("Project Settings")
                                .on_press(store::open_project_settings)
                                .px(px(10.0))
                                .py(px(5.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text("Project Settings")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                ),
        )
        .child(view().flex_row().items_center().gap(px(8.0)).child(each(
            store::workflow_ids,
            |id: &WorkflowId| *id,
            |id, _| workflow_pill(id.get_untracked()),
        )))
        .child(
            text(move || {
                store::selected_workflow()
                    .map(|w| {
                        let steps = w
                            .steps
                            .iter()
                            .enumerate()
                            .map(|(i, s)| format!("{}. {} ({})", i + 1, s.name, s.kind.label()))
                            .collect::<Vec<_>>()
                            .join("  →  ");
                        format!("{} — {}", w.description, steps)
                    })
                    .unwrap_or_default()
            })
            .text_size(px(12.0))
            .text_color(palette::text_muted()),
        )
}

fn workflow_pill(id: WorkflowId) -> impl IntoElement {
    let is_selected = move || store::selected_workflow_id() == Some(id);
    button()
        .on_press(move || store::select_workflow(id))
        .px(px(11.0))
        .py(px(6.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(move || {
            if is_selected() {
                palette::nav_active_bg()
            } else {
                palette::border_color()
            }
        })
        .bg(move || {
            if is_selected() {
                palette::nav_active_bg()
            } else {
                palette::control_bg()
            }
        })
        .active(|s| s.opacity(0.8))
        .child(
            text(move || {
                store::workflow_by_id(id)
                    .map(|w| w.name)
                    .unwrap_or_default()
            })
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || {
                if is_selected() {
                    palette::nav_active_fg()
                } else {
                    palette::text_primary()
                }
            }),
        )
}

fn launch_bar(
    task_prompt: Signal<String>,
    submit_run: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .child(
            input()
                .value(move || task_prompt.get())
                .on_change(move |value: &str| task_prompt.set(value.to_owned()))
                .on_submit(move |_: &str| submit_run())
                .placeholder("Describe work for the AI workflow to execute...")
                .flex_1()
                .px(px(12.0))
                .py(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_bg())
                .rounded(px(8.0))
                .text_color(palette::text_primary()),
        )
        .child(
            button()
                .disabled(move || {
                    task_prompt.with(|prompt| prompt.trim().is_empty())
                        || store::selected_project_id().is_none()
                        || store::selected_workflow_id().is_none()
                })
                .on_press(submit_run)
                .px(px(14.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .bg(palette::nav_active_bg())
                .active(|s| s.opacity(0.8))
                .disabled_style(|s| s.opacity(0.4))
                .child(
                    text("Run Workflow")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        )
}

fn runs_list_panel() -> impl IntoElement {
    view()
        .w(px(290.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            text("Workflow runs")
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            scroll().flex_1().child(
                view()
                    .gap(px(8.0))
                    .child(when(
                        || store::run_ids_for_selected_project().is_empty(),
                        || {
                            text("No runs yet. Enter a task above and press Run Workflow.")
                                .text_size(px(12.0))
                                .text_color(palette::text_muted())
                        },
                    ))
                    .child(each(
                        store::run_ids_for_selected_project,
                        |id: &RunId| *id,
                        |id, _| run_row(id.get_untracked()),
                    )),
            ),
        )
}

fn run_row(id: RunId) -> impl IntoElement {
    let is_selected = move || store::selected_run_id() == Some(id);
    button()
        .on_press(move || store::select_run(id))
        .p(px(10.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(move || {
            if is_selected() {
                palette::nav_active_bg()
            } else {
                palette::border_color()
            }
        })
        .bg(move || {
            if is_selected() {
                palette::subtle_surface_bg()
            } else {
                palette::card_inset_bg()
            }
        })
        .gap(px(4.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(move || {
                        store::run_by_id(id)
                            .map(|r| format!("#{} {}", r.id, r.workflow_name))
                            .unwrap_or_default()
                    })
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_primary()),
                )
                .child(
                    text(move || {
                        store::run_by_id(id)
                            .map(|r| r.status.label())
                            .unwrap_or_default()
                    })
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                ),
        )
        .child(
            text(move || {
                store::run_by_id(id)
                    .map(|r| r.task_prompt)
                    .unwrap_or_default()
            })
            .text_size(px(11.5))
            .text_color(palette::text_muted()),
        )
}

fn run_inspector_panel() -> impl IntoElement {
    view()
        .flex_1()
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .p(px(16.0))
        .gap(px(14.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("Run inspector")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(8.0))
                        .child(
                            button()
                                .disabled(move || {
                                    store::selected_run().is_none_or(|run| run.status.is_terminal())
                                })
                                .on_press(store::advance_selected_run)
                                .px(px(11.0))
                                .py(px(5.0))
                                .rounded(px(7.0))
                                .bg(palette::nav_active_bg())
                                .active(|s| s.opacity(0.8))
                                .disabled_style(|s| s.opacity(0.4))
                                .child(
                                    text(move || match store::selected_run().map(|r| r.status) {
                                        Some(RunStatus::WaitingForApproval) => "Approve Gate",
                                        _ => "Advance Step",
                                    })
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::nav_active_fg()),
                                ),
                        )
                        .child(
                            button()
                                .disabled(move || {
                                    store::selected_run().is_none_or(|run| run.status.is_terminal())
                                })
                                .on_press(store::cancel_selected_run)
                                .px(px(11.0))
                                .py(px(5.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(move || theme().destructive)
                                .active(|s| s.opacity(0.75))
                                .disabled_style(|s| s.opacity(0.4))
                                .child(
                                    text("Cancel Run")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || theme().destructive),
                                ),
                        ),
                ),
        )
        .child(
            scroll().flex_1().child(
                view()
                    .gap(px(12.0))
                    .child(
                        text(move || match store::selected_run() {
                            Some(run) => format!(
                                "Run #{} · {} ({}/{})",
                                run.id,
                                run.status.label(),
                                run.completed_steps(),
                                run.steps.len()
                            ),
                            None => "Select or start a workflow run to inspect steps and logs."
                                .to_owned(),
                        })
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                    )
                    .child(
                        text(move || {
                            store::selected_run()
                                .map(|run| {
                                    run.steps
                                        .iter()
                                        .enumerate()
                                        .map(|(index, step)| {
                                            let marker = match step.status {
                                                StepRunStatus::Succeeded => "[done]",
                                                StepRunStatus::Running => "[running]",
                                                StepRunStatus::Failed => "[failed]",
                                                StepRunStatus::Skipped => "[skipped]",
                                                StepRunStatus::Pending => "[pending]",
                                            };
                                            format!(
                                                "{}. {} {} ({})",
                                                index + 1,
                                                marker,
                                                step.step_name,
                                                step.kind.label()
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default()
                        })
                        .text_size(px(13.0))
                        .text_color(palette::text_primary()),
                    )
                    .child(
                        text(move || {
                            store::selected_run()
                                .map(|run| {
                                    run.logs
                                        .iter()
                                        .map(|entry| {
                                            format!("[{}] {}", entry.step_name, entry.message)
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default()
                        })
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                    ),
            ),
        )
}
