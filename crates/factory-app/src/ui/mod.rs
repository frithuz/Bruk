//! Main macOS desktop layout: project & workflow sidebar + execution workspace.

use factory_core::{ProjectId, RunId, RunStatus, StepRunStatus, WorkflowId};
use nsh_base::{button, input, theme};
use nsh_core::prelude::*;

use crate::store;

/// Top-level split workspace: left navigation rail and right workflow canvas.
pub(crate) fn workspace() -> impl IntoElement {
    view()
        .flex_1()
        .flex_row()
        .bg(move || theme().background)
        .child(sidebar())
        .child(main_panel())
}

fn sidebar() -> impl IntoElement {
    let new_name = signal(String::new());
    let new_path = signal(String::new());

    let submit_project = move || {
        let name = new_name.get_untracked();
        let path = new_path.get_untracked();
        if store::add_project(&name, &path, "main") {
            new_name.set(String::new());
            new_path.set(String::new());
        }
    };

    view()
        .w(px(280.0))
        .bg(move || theme().sidebar)
        .border_r_1()
        .border_color(move || theme().sidebar_border)
        .p_4()
        .gap_4()
        .child(
            view()
                .gap_1()
                .child(
                    text("Software Factory")
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme().sidebar_foreground),
                )
                .child(
                    text("AI Workflow Orchestrator")
                        .text_xs()
                        .text_color(move || theme().muted_foreground),
                ),
        )
        .child(
            scroll()
                .flex_1()
                .child(
                    view()
                        .gap_5()
                        .child(
                            view()
                                .gap_2()
                                .child(section_label("PROJECTS"))
                                .child(each(
                                    store::project_ids,
                                    |id: &ProjectId| *id,
                                    |id, _| project_row(id.get_untracked()),
                                )),
                        )
                        .child(
                            view()
                                .gap_2()
                                .p_3()
                                .rounded(move || theme().radius)
                                .border_1()
                                .border_color(move || theme().sidebar_border)
                                .bg(move || theme().card)
                                .child(section_label("ADD PROJECT"))
                                .child(
                                    input()
                                        .value(move || new_name.get())
                                        .on_change(move |value: &str| {
                                            new_name.set(value.to_owned())
                                        })
                                        .placeholder("Project name")
                                        .px_3()
                                        .py_2()
                                        .border_1()
                                        .border_color(move || theme().input)
                                        .rounded(move || theme().radius)
                                        .text_sm()
                                        .text_color(move || theme().foreground),
                                )
                                .child(
                                    input()
                                        .value(move || new_path.get())
                                        .on_change(move |value: &str| {
                                            new_path.set(value.to_owned())
                                        })
                                        .on_submit(move |_: &str| submit_project())
                                        .placeholder("Repository path")
                                        .px_3()
                                        .py_2()
                                        .border_1()
                                        .border_color(move || theme().input)
                                        .rounded(move || theme().radius)
                                        .text_sm()
                                        .text_color(move || theme().foreground),
                                )
                                .child(
                                    button()
                                        .disabled(move || {
                                            new_name.with(|n| n.trim().is_empty())
                                                || new_path.with(|p| p.trim().is_empty())
                                        })
                                        .on_press(submit_project)
                                        .px_3()
                                        .py_2()
                                        .rounded(move || theme().radius)
                                        .bg(move || theme().secondary)
                                        .active(|s| s.opacity(0.75))
                                        .disabled_style(|s| s.opacity(0.45))
                                        .items_center()
                                        .child(
                                            text("Add Project")
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(move || theme().secondary_foreground),
                                        ),
                                ),
                        )
                        .child(
                            view()
                                .gap_2()
                                .child(section_label("WORKFLOWS"))
                                .child(each(
                                    store::workflow_ids,
                                    |id: &WorkflowId| *id,
                                    |id, _| workflow_row(id.get_untracked()),
                                )),
                        ),
                ),
        )
}

fn project_row(id: ProjectId) -> impl IntoElement {
    let is_selected = move || store::selected_project_id() == Some(id);
    button()
        .on_press(move || store::select_project(id))
        .px_3()
        .py_2()
        .rounded(move || theme().radius)
        .bg(move || {
            if is_selected() {
                theme().sidebar_accent
            } else {
                AdaptiveColor::from(transparent_black())
            }
        })
        .active(|s| s.opacity(0.75))
        .gap_1()
        .child(
            text(move || {
                store::project_by_id(id)
                    .map(|p| p.name)
                    .unwrap_or_default()
            })
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || {
                if is_selected() {
                    theme().sidebar_accent_foreground
                } else {
                    theme().sidebar_foreground
                }
            }),
        )
        .child(
            text(move || {
                store::project_by_id(id)
                    .map(|p| format!("{} · {}", p.default_branch, p.root_path.display()))
                    .unwrap_or_default()
            })
            .text_xs()
            .text_color(move || theme().muted_foreground),
        )
}

fn workflow_row(id: WorkflowId) -> impl IntoElement {
    let is_selected = move || store::selected_workflow_id() == Some(id);
    button()
        .on_press(move || store::select_workflow(id))
        .px_3()
        .py_2()
        .rounded(move || theme().radius)
        .bg(move || {
            if is_selected() {
                theme().sidebar_accent
            } else {
                AdaptiveColor::from(transparent_black())
            }
        })
        .active(|s| s.opacity(0.75))
        .gap_1()
        .child(
            text(move || {
                store::workflow_by_id(id)
                    .map(|w| w.name)
                    .unwrap_or_default()
            })
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || {
                if is_selected() {
                    theme().sidebar_accent_foreground
                } else {
                    theme().sidebar_foreground
                }
            }),
        )
        .child(
            text(move || {
                store::workflow_by_id(id)
                    .map(|w| format!("{} steps", w.steps.len()))
                    .unwrap_or_default()
            })
            .text_xs()
            .text_color(move || theme().muted_foreground),
        )
}

fn main_panel() -> impl IntoElement {
    let task_prompt = signal(String::new());
    let submit_run = move || {
        let prompt = task_prompt.get_untracked();
        if store::launch_run(&prompt) {
            task_prompt.set(String::new());
        }
    };

    view()
        .flex_1()
        .p_6()
        .gap_5()
        .child(header_card())
        .child(launch_bar(task_prompt, submit_run))
        .child(
            view()
                .flex_1()
                .flex_row()
                .gap_4()
                .child(runs_list_panel())
                .child(run_inspector_panel()),
        )
        .child(status_bar())
}

fn header_card() -> impl IntoElement {
    view()
        .p_4()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .gap_2()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(move || {
                        store::selected_project()
                            .map(|p| p.name)
                            .unwrap_or_else(|| "No project selected".to_owned())
                    })
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || theme().card_foreground),
                )
                .child(
                    text(move || {
                        store::selected_workflow()
                            .map(|w| format!("Workflow: {}", w.name))
                            .unwrap_or_else(|| "Select a workflow".to_owned())
                    })
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || theme().muted_foreground),
                ),
        )
        .child(
            text(move || {
                store::selected_workflow()
                    .map(|w| w.description)
                    .unwrap_or_default()
            })
            .text_sm()
            .text_color(move || theme().muted_foreground),
        )
        .child(
            text(move || {
                store::selected_workflow()
                    .map(|w| {
                        w.steps
                            .iter()
                            .enumerate()
                            .map(|(i, s)| format!("{}. {} ({})", i + 1, s.name, s.kind.label()))
                            .collect::<Vec<_>>()
                            .join("  →  ")
                    })
                    .unwrap_or_default()
            })
            .text_xs()
            .text_color(move || theme().muted_foreground),
        )
}

fn launch_bar(
    task_prompt: Signal<String>,
    submit_run: impl Fn() + Copy + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            input()
                .value(move || task_prompt.get())
                .on_change(move |value: &str| task_prompt.set(value.to_owned()))
                .on_submit(move |_: &str| submit_run())
                .placeholder("Describe work for the AI workflow to execute...")
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
                .disabled(move || {
                    task_prompt.with(|prompt| prompt.trim().is_empty())
                        || store::selected_project_id().is_none()
                        || store::selected_workflow_id().is_none()
                })
                .on_press(submit_run)
                .px_4()
                .py_2()
                .rounded(move || theme().radius)
                .bg(move || theme().primary)
                .active(|s| s.opacity(0.8))
                .disabled_style(|s| s.opacity(0.4))
                .child(
                    text("Run Workflow")
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme().primary_foreground),
                ),
        )
}

fn runs_list_panel() -> impl IntoElement {
    view()
        .w(px(280.0))
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .p_3()
        .gap_3()
        .child(section_label("WORKFLOW RUNS"))
        .child(
            scroll().flex_1().child(
                view()
                    .gap_2()
                    .child(when(
                        || store::run_ids_for_selected_project().is_empty(),
                        || {
                            text("No runs yet. Enter a task above and press Run Workflow.")
                                .text_xs()
                                .text_color(move || theme().muted_foreground)
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
        .p_3()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || {
            if is_selected() {
                theme().ring
            } else {
                theme().border
            }
        })
        .bg(move || {
            if is_selected() {
                theme().accent
            } else {
                theme().background
            }
        })
        .gap_1()
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
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(move || theme().foreground),
                )
                .child(
                    text(move || {
                        store::run_by_id(id)
                            .map(|r| r.status.label())
                            .unwrap_or_default()
                    })
                    .text_xs()
                    .text_color(move || theme().muted_foreground),
                ),
        )
        .child(
            text(move || {
                store::run_by_id(id)
                    .map(|r| r.task_prompt)
                    .unwrap_or_default()
            })
            .text_xs()
            .text_color(move || theme().muted_foreground),
        )
}

fn run_inspector_panel() -> impl IntoElement {
    view()
        .flex_1()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .p_4()
        .gap_4()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(section_label("RUN INSPECTOR"))
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .child(
                            button()
                                .disabled(move || {
                                    store::selected_run()
                                        .is_none_or(|run| run.status.is_terminal())
                                })
                                .on_press(store::advance_selected_run)
                                .px_3()
                                .py_1()
                                .rounded(move || theme().radius)
                                .bg(move || theme().primary)
                                .active(|s| s.opacity(0.8))
                                .disabled_style(|s| s.opacity(0.4))
                                .child(
                                    text(move || match store::selected_run().map(|r| r.status) {
                                        Some(RunStatus::WaitingForApproval) => "Approve Gate",
                                        _ => "Advance Step",
                                    })
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(move || theme().primary_foreground),
                                ),
                        )
                        .child(
                            button()
                                .disabled(move || {
                                    store::selected_run()
                                        .is_none_or(|run| run.status.is_terminal())
                                })
                                .on_press(store::cancel_selected_run)
                                .px_3()
                                .py_1()
                                .rounded(move || theme().radius)
                                .border_1()
                                .border_color(move || theme().destructive)
                                .active(|s| s.opacity(0.75))
                                .disabled_style(|s| s.opacity(0.4))
                                .child(
                                    text("Cancel Run")
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || theme().destructive),
                                ),
                        ),
                ),
        )
        .child(
            scroll().flex_1().child(
                view()
                    .gap_4()
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
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || theme().card_foreground),
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
                        .text_sm()
                        .text_color(move || theme().foreground),
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
                        .text_xs()
                        .text_color(move || theme().muted_foreground),
                    ),
            ),
        )
}

fn status_bar() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .pt_2()
        .border_t_1()
        .border_color(move || theme().border)
        .child(
            text(move || {
                store::status_message().unwrap_or_else(|| "Ready".to_owned())
            })
            .text_xs()
            .text_color(move || theme().muted_foreground),
        )
        .child(
            text("Native Shell · macOS AppKit")
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
}

fn section_label(label: &'static str) -> impl IntoElement {
    text(label)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(move || theme().muted_foreground)
}
