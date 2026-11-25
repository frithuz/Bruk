//! Live Workflow Run view: run header, 5-step pipeline strip, Output/Diff/Coverage tabs,
//! transcript tool cards, steering composer, and right-hand step inspector matching `02 Main.png`.

use std::collections::BTreeSet;

use factory_core::{
    CoverageFileRow, DiffFileChange, DiffLineKind, RunDetailTab, RunExecutionState, RunStepDetail,
    RunStepStatus, StepTranscriptEntry, TerminalBadgeTone, TerminalLine, ToolStatusTone,
    WorkflowRunViewData,
};
use nsh_base::{button, input};
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{icons, palette},
};

const INSPECTOR_WIDTH: f32 = 286.0;

/// Main Workflow Run split canvas (run detail + steering composer on left, step inspector on right).
pub(crate) fn workflow_run_view() -> impl IntoElement {
    let steer_draft = signal(String::new());
    let attempt_details_open = signal(false);
    let toggled_tool_cards = signal(BTreeSet::<(usize, usize)>::new());
    let command_query = signal(String::new());

    view()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .flex_row()
        .bg(palette::canvas_bg())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .child(when(store::workflow_run_command_palette_open, move || {
                    command_palette_bar(command_query)
                }))
                .child(scroll().flex_1().min_h(px(0.0)).child(dynamic(move || {
                    let run = store::active_workflow_run();
                    if run.is_empty() {
                        return workflow_run_empty_state(&run).into_element();
                    }
                    let selected_idx =
                        store::workflow_run_selected_step().min(run.steps.len().saturating_sub(1));
                    let active_tab = store::workflow_run_tab();
                    let attempt_open = attempt_details_open.get();
                    let toggled_cards = toggled_tool_cards.get();

                    let mut canvas = view()
                        .flex_1()
                        .px(px(22.0))
                        .pt(px(18.0))
                        .pb(px(16.0))
                        .gap(px(14.0))
                        .child(run_header(&run))
                        .child(step_pipeline_strip(&run, selected_idx))
                        .child(detail_tab_bar(&run, active_tab));

                    canvas = match active_tab {
                        RunDetailTab::Output => {
                            if let Some(step) = run.steps.get(selected_idx) {
                                canvas.child(output_tab_view(
                                    &run,
                                    step,
                                    selected_idx,
                                    attempt_open,
                                    attempt_details_open,
                                    &toggled_cards,
                                    toggled_tool_cards,
                                ))
                            } else {
                                canvas
                            }
                        }
                        RunDetailTab::Diff => canvas.child(diff_tab_view(&run)),
                        RunDetailTab::Coverage => canvas.child(coverage_tab_view(&run)),
                    };

                    canvas.into_element()
                })))
                .child(when(
                    || !store::active_workflow_run().is_empty(),
                    move || steering_composer_bar(steer_draft),
                )),
        )
        .child(when(
            || store::workflow_run_inspector_visible() && !store::active_workflow_run().is_empty(),
            step_inspector_panel,
        ))
}

fn workflow_run_empty_state(run: &WorkflowRunViewData) -> impl IntoElement {
    let title = if run.workflow_name.trim().is_empty() {
        "No workflow runs yet".to_owned()
    } else {
        format!("No runs for '{}' yet", run.workflow_name)
    };
    view()
        .test_id("wf-run-empty-state")
        .flex_1()
        .items_center()
        .justify_center()
        .py(px(72.0))
        .px(px(24.0))
        .gap(px(10.0))
        .child(
            text(title)
                .text_size(px(17.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            text("Start an ad-hoc run or queue an issue to see live step output, diffs, and execution progress here.")
                .text_center()
                .w(px(420.0))
                .text_size(px(13.0))
                .text_color(palette::text_muted()),
        )
        .child(
            view()
                .pt(px(4.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    button()
                        .on_press(store::edit_active_workflow)
                        .h(px(32.0))
                        .px(px(14.0))
                        .rounded(px(8.0))
                        .bg(palette::nav_active_bg())
                        .items_center()
                        .justify_center()
                        .child(
                            text("Edit Workflow")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                )
                .child(
                    button()
                        .on_press(store::open_work_queue)
                        .h(px(32.0))
                        .px(px(12.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .items_center()
                        .justify_center()
                        .child(
                            text("Open Work Queue")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
}

// --- Command Palette Inline Sheet -------------------------------------------------------

fn command_palette_bar(query: Signal<String>) -> impl IntoElement {
    view()
        .mx(px(22.0))
        .mt(px(12.0))
        .p(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::card_selected_border())
        .bg(palette::card_bg())
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(icons::search(palette::text_muted()))
                .child(
                    input()
                        .test_id("wf-run-command-input")
                        .value(move || query.get())
                        .on_change(move |value: &str| query.set(value.to_owned()))
                        .placeholder("Type a command or jump to a tab…")
                        .flex_1()
                        .px(px(8.0))
                        .py(px(5.0))
                        .rounded(px(6.0))
                        .bg(palette::subtle_surface_bg())
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .accessibility_label("Close Command Palette")
                        .on_press(|| store::set_workflow_run_command_palette_open(false))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(6.0))
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Esc")
                                .text_size(px(11.0))
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(dynamic(move || {
            let filter = query.get().trim().to_ascii_lowercase();
            let commands: Vec<(&str, &str, fn())> = vec![
                ("Switch to Output tab", "⌘1", || {
                    store::set_workflow_run_tab(RunDetailTab::Output);
                    store::set_workflow_run_command_palette_open(false);
                }),
                ("Switch to Diff tab", "⌘2", || {
                    store::set_workflow_run_tab(RunDetailTab::Diff);
                    store::set_workflow_run_command_palette_open(false);
                }),
                ("Switch to Coverage tab", "⌘4", || {
                    store::set_workflow_run_tab(RunDetailTab::Coverage);
                    store::set_workflow_run_command_palette_open(false);
                }),
                ("Pause or Resume Run", "⌘.", || {
                    store::toggle_pause_workflow_run();
                    store::set_workflow_run_command_palette_open(false);
                }),
                ("Toggle Step Inspector", "⌥⌘I", || {
                    store::toggle_workflow_run_inspector();
                    store::set_workflow_run_command_palette_open(false);
                }),
            ];

            let mut list = view().flex_row().flex_wrap().gap(px(6.0));
            for (label, shortcut, action) in commands {
                if !filter.is_empty() && !label.to_ascii_lowercase().contains(&filter) {
                    continue;
                }
                list = list.child(
                    button()
                        .on_press(action)
                        .px(px(10.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(label)
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(shortcut)
                                .text_size(px(11.0))
                                .text_color(palette::text_muted()),
                        ),
                );
            }
            list
        }))
}

// --- Run Header & Pipeline Strip --------------------------------------------------------

fn run_header(run: &WorkflowRunViewData) -> impl IntoElement {
    let (pill_bg, pill_fg) = match run.execution_state {
        RunExecutionState::Running => (palette::status_running_bg(), palette::link_blue()),
        RunExecutionState::Paused | RunExecutionState::NeedsApproval => {
            (palette::status_warning_bg(), palette::review_amber())
        }
        RunExecutionState::Succeeded => (palette::status_success_bg(), palette::success_text()),
        RunExecutionState::Failed | RunExecutionState::Stopped => {
            (palette::status_error_bg(), palette::destructive_text())
        }
    };

    let pill_icon = match run.execution_state {
        RunExecutionState::Running => icons::status_running(pill_fg).into_element(),
        RunExecutionState::Paused => icons::pause(pill_fg).into_element(),
        RunExecutionState::NeedsApproval => icons::shield_approval(pill_fg).into_element(),
        RunExecutionState::Succeeded => icons::status_merged(pill_fg).into_element(),
        RunExecutionState::Failed => icons::alert_circle(pill_fg).into_element(),
        RunExecutionState::Stopped => icons::stop_square(pill_fg).into_element(),
    };

    view()
        .gap(px(5.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(format!("Run #{}", run.run_number))
                        .text_size(px(21.0))
                        .font_weight(FontWeight::BOLD)
                        .tabular_nums()
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(5.5))
                        .px(px(10.0))
                        .py(px(3.5))
                        .rounded_full()
                        .bg(pill_bg)
                        .child(pill_icon)
                        .child(
                            text(run.status_pill_label())
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .tabular_nums()
                                .text_color(pill_fg),
                        ),
                ),
        )
        .child(
            text(run.subtitle.clone())
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
}

fn step_pipeline_strip(run: &WorkflowRunViewData, selected_idx: usize) -> impl IntoElement {
    let mut row = view().flex_row().items_stretch().gap(px(8.0));
    for (idx, step) in run.steps.iter().enumerate() {
        row = row.child(step_card(step, idx, idx == selected_idx));
    }
    row
}

fn step_card(step: &RunStepDetail, index: usize, selected: bool) -> impl IntoElement {
    let (status_color, status_icon) = match step.status {
        RunStepStatus::Done => (
            palette::success_text(),
            icons::status_merged(palette::success_text()).into_element(),
        ),
        RunStepStatus::Running => (
            palette::link_blue(),
            icons::status_running(palette::link_blue()).into_element(),
        ),
        RunStepStatus::Paused => (
            palette::review_amber(),
            icons::pause(palette::review_amber()).into_element(),
        ),
        RunStepStatus::NeedsApproval => (
            palette::review_amber(),
            icons::shield_approval(palette::review_amber()).into_element(),
        ),
        RunStepStatus::Queued => (
            palette::text_muted(),
            icons::status_queued(palette::text_muted()).into_element(),
        ),
        RunStepStatus::Failed => (
            palette::destructive_text(),
            icons::alert_circle(palette::destructive_text()).into_element(),
        ),
        RunStepStatus::Stopped => (
            palette::text_muted(),
            icons::stop_square(palette::text_muted()).into_element(),
        ),
    };

    let duration_label = step.duration.clone().unwrap_or_else(|| "—".to_owned());
    let card_test_id = format!("wf-step-card-{}", step.number);
    let card_a11y = format!("{}. {}", step.number, step.title);

    let mut top_section = view()
        .gap(px(5.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(4.5))
                        .min_w(px(0.0))
                        .child(status_icon)
                        .child(
                            text(step.status.label())
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(status_color),
                        ),
                )
                .child(
                    text(duration_label)
                        .text_size(px(11.0))
                        .tabular_nums()
                        .text_color(if selected {
                            palette::text_secondary()
                        } else {
                            palette::text_muted()
                        }),
                ),
        )
        .child(
            text(format!("{}. {}", step.number, step.title))
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        );

    if let Some(loop_text) = &step.loop_badge {
        let loop_color = if step.status == RunStepStatus::Failed {
            palette::destructive_text()
        } else {
            palette::link_blue()
        };
        top_section = top_section.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(4.5))
                .child(icons::loop_arrows(loop_color))
                .child(
                    text(loop_text.clone())
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(loop_color),
                ),
        );
    }

    button()
        .test_id(&card_test_id)
        .accessibility_label(card_a11y)
        .on_press(move || store::select_workflow_run_step(index))
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(96.0))
        .p(px(10.0))
        .rounded(px(9.5))
        .border_1()
        .border_color(if selected {
            palette::card_selected_border()
        } else {
            palette::border_color()
        })
        .bg(if selected {
            palette::card_selected_bg()
        } else {
            palette::card_bg()
        })
        .justify_between()
        .gap(px(8.0))
        .hover(move |s| {
            if selected {
                s
            } else {
                s.bg(palette::card_inset_bg())
            }
        })
        .active(|s| s.opacity(0.88))
        .child(top_section)
        .child(
            view().flex_row().child(
                view()
                    .px(px(7.0))
                    .py(px(2.0))
                    .rounded(px(5.5))
                    .border_1()
                    .border_color(if selected {
                        palette::intake_banner_border()
                    } else {
                        palette::divider_color()
                    })
                    .bg(if selected {
                        palette::card_bg()
                    } else {
                        palette::tag_pill_bg()
                    })
                    .child(
                        text(step.actor_badge.clone())
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_muted()),
                    ),
            ),
        )
}

// --- Sub-Tab Bar ------------------------------------------------------------------------

fn detail_tab_bar(run: &WorkflowRunViewData, active_tab: RunDetailTab) -> impl IntoElement {
    let diff_count = run.diffs.len();

    view()
        .flex_row()
        .items_center()
        .gap(px(18.0))
        .border_b_1()
        .border_color(palette::border_color())
        .child(tab_button(
            "wf-tab-output",
            RunDetailTab::Output,
            active_tab == RunDetailTab::Output,
            None,
            false,
        ))
        .child(tab_button(
            "wf-tab-diff",
            RunDetailTab::Diff,
            active_tab == RunDetailTab::Diff,
            Some(diff_count),
            false,
        ))
        .child(tab_button(
            "wf-tab-coverage",
            RunDetailTab::Coverage,
            active_tab == RunDetailTab::Coverage,
            None,
            true,
        ))
}

fn tab_button(
    test_id: &'static str,
    tab: RunDetailTab,
    is_active: bool,
    badge_count: Option<usize>,
    is_extension: bool,
) -> impl IntoElement {
    let mut row = view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .pb(px(9.0))
        .pt(px(2.0));

    if is_extension {
        row = row.child(icons::extension_puzzle(
            12.5,
            if is_active {
                palette::text_primary()
            } else {
                palette::text_muted()
            },
        ));
    }

    row = row.child(
        text(tab.label())
            .text_size(px(12.5))
            .font_weight(if is_active {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            })
            .text_color(if is_active {
                palette::text_primary()
            } else {
                palette::text_muted()
            }),
    );

    if let Some(count) = badge_count {
        row = row.child(
            view()
                .min_w(px(19.0))
                .px(px(6.0))
                .py(px(1.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .bg(palette::queue_badge_bg())
                .child(
                    text(count.to_string())
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .tabular_nums()
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    button()
        .test_id(test_id)
        .accessibility_label(tab.label())
        .on_press(move || store::set_workflow_run_tab(tab))
        .hover(|s| s.opacity(0.85))
        .active(|s| s.opacity(0.75))
        .child(
            view()
                .child(row)
                .child(view().h(px(2.0)).w_full().rounded_full().bg(if is_active {
                    palette::text_primary()
                } else {
                    AdaptiveColor::TRANSPARENT
                })),
        )
}

// --- Output Tab -------------------------------------------------------------------------

fn output_tab_view(
    run: &WorkflowRunViewData,
    step: &RunStepDetail,
    step_index: usize,
    attempt_open: bool,
    attempt_details_open: Signal<bool>,
    toggled_cards: &BTreeSet<(usize, usize)>,
    toggled_tool_cards: Signal<BTreeSet<(usize, usize)>>,
) -> impl IntoElement {
    let mut stack = view().gap(px(12.0));

    let badge_label = step
        .transcript
        .iter()
        .find_map(|entry| match entry {
            StepTranscriptEntry::AgentTurn { badge, .. } => Some(badge.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "Cx".to_owned());

    let mut stream_col = view().flex_1().min_w(px(0.0)).gap(px(10.0));

    for (entry_idx, entry) in step.transcript.iter().enumerate() {
        match entry {
            StepTranscriptEntry::AgentTurn {
                attempt_banner,
                body,
                ..
            } => {
                if let Some(banner) = attempt_banner {
                    let banner_summary = banner.summary.clone();
                    let banner_details = banner.details.clone();
                    stream_col = stream_col.child(
                        view().gap(px(6.0)).child(
                            view().flex_row().child(
                                button()
                                    .test_id("wf-attempt-banner")
                                    .accessibility_label(banner_summary.clone())
                                    .on_press(move || attempt_details_open.set(!attempt_open))
                                    .px(px(9.0))
                                    .py(px(3.5))
                                    .rounded(px(6.0))
                                    .bg(palette::attempt_banner_bg())
                                    .border_1()
                                    .border_color(palette::attempt_banner_border())
                                    .hover(|s| s.opacity(0.88))
                                    .child(
                                        text(banner_summary)
                                            .text_size(px(11.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(palette::attempt_banner_text()),
                                    ),
                            ),
                        ),
                    );
                    if attempt_open {
                        stream_col = stream_col.child(
                            view()
                                .px(px(11.0))
                                .py(px(8.0))
                                .rounded(px(8.0))
                                .bg(palette::attempt_banner_bg())
                                .border_1()
                                .border_color(palette::attempt_banner_border())
                                .child(
                                    text(banner_details)
                                        .app_class("font-mono")
                                        .text_size(px(11.5))
                                        .text_color(palette::attempt_banner_text()),
                                ),
                        );
                    }
                }
                stream_col = stream_col.child(
                    text(body.clone())
                        .text_size(px(13.0))
                        .text_color(palette::text_primary()),
                );
            }
            StepTranscriptEntry::ToolCall {
                verb,
                target,
                status_text,
                status_tone,
                output_lines,
                expanded,
            } => {
                let is_toggled = toggled_cards.contains(&(step_index, entry_idx));
                let is_expanded = *expanded ^ is_toggled;
                stream_col = stream_col.child(tool_call_card(
                    step_index,
                    entry_idx,
                    verb,
                    target,
                    status_text,
                    *status_tone,
                    output_lines,
                    is_expanded,
                    toggled_tool_cards,
                ));
            }
            StepTranscriptEntry::McpProblem { problem } => {
                stream_col = stream_col.child(crate::ui::mcp::mcp_problem_banner(
                    problem.clone(),
                    Some(run.project_name.clone()),
                ));
            }
            StepTranscriptEntry::SecretProblem { problem } => {
                stream_col = stream_col.child(crate::ui::secrets::secret_problem_banner(
                    problem.clone(),
                    Some(run.project_name.clone()),
                ));
            }
            StepTranscriptEntry::UserSteering { message } => {
                stream_col = stream_col.child(
                    view()
                        .p(px(10.0))
                        .rounded(px(9.0))
                        .border_1()
                        .border_color(palette::intake_banner_border())
                        .bg(palette::intake_banner_bg())
                        .gap(px(3.0))
                        .child(
                            text("You steered this step")
                                .text_size(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::link_blue()),
                        )
                        .child(
                            text(message.clone())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                );
            }
        }
    }

    if step.status == RunStepStatus::NeedsApproval && !run.execution_state.is_terminal() {
        stream_col = stream_col.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .pt(px(4.0))
                .child(
                    button()
                        .test_id("wf-approve-gate-btn")
                        .accessibility_label("Approve Review Gate")
                        .on_press(store::approve_workflow_run_review_gate)
                        .px(px(14.0))
                        .py(px(7.0))
                        .rounded(px(7.5))
                        .bg(palette::nav_active_bg())
                        .active(|s| s.opacity(0.85))
                        .child(
                            text("Approve & Continue")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        );
    }

    stack = stack.child(
        view()
            .flex_row()
            .items_start()
            .gap(px(10.0))
            .child(
                view()
                    .w(px(24.0))
                    .h(px(24.0))
                    .mt(px(1.0))
                    .flex_shrink_0()
                    .rounded(px(6.0))
                    .bg(palette::subtle_surface_bg())
                    .border_1()
                    .border_color(palette::divider_color())
                    .items_center()
                    .justify_center()
                    .child(
                        text(badge_label)
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_secondary()),
                    ),
            )
            .child(stream_col),
    );

    stack
}

#[allow(clippy::too_many_arguments)]
fn tool_call_card(
    step_index: usize,
    entry_idx: usize,
    verb: &str,
    target: &str,
    status_text: &str,
    status_tone: ToolStatusTone,
    output_lines: &[TerminalLine],
    expanded: bool,
    toggled_tool_cards: Signal<BTreeSet<(usize, usize)>>,
) -> impl IntoElement {
    let status_color = match status_tone {
        ToolStatusTone::Neutral => palette::text_secondary(),
        ToolStatusTone::Success => palette::success_text(),
        ToolStatusTone::Running => palette::link_blue(),
        ToolStatusTone::Error => palette::destructive_text(),
    };
    let has_lines = !output_lines.is_empty();
    let card_test_id = format!("wf-tool-card-{step_index}-{entry_idx}");

    let header = button()
        .test_id(&card_test_id)
        .accessibility_label(format!("{verb} {target}"))
        .on_press(move || {
            if has_lines {
                toggled_tool_cards.update(|set| {
                    let key = (step_index, entry_idx);
                    if !set.remove(&key) {
                        set.insert(key);
                    }
                });
            }
        })
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(12.0))
        .py(px(9.0))
        .hover(move |s| {
            if has_lines {
                s.bg(palette::card_inset_bg())
            } else {
                s
            }
        })
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(7.5))
                .child(icons::terminal_prompt(palette::text_muted()))
                .child(
                    text(verb.to_owned())
                        .text_size(px(12.5))
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(target.to_owned())
                        .app_class("font-mono")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            text(status_text.to_owned())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .text_color(status_color),
        );

    let mut card = view()
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(header);

    if expanded && has_lines {
        let mut lines_box = view()
            .border_t_1()
            .border_color(palette::divider_color())
            .bg(palette::card_inset_bg())
            .px(px(14.0))
            .py(px(11.0))
            .gap(px(4.5));

        for line in output_lines {
            lines_box = lines_box.child(terminal_output_row(line));
        }

        card = card.child(lines_box);
    }

    card
}

fn terminal_output_row(line: &TerminalLine) -> impl IntoElement {
    let mut row = view().flex_row().items_center().gap(px(10.0));

    if let Some(badge) = &line.badge {
        let color = match line.badge_tone {
            TerminalBadgeTone::Pass => palette::success_text(),
            TerminalBadgeTone::Runs => palette::text_secondary(),
            TerminalBadgeTone::Fail => palette::destructive_text(),
            TerminalBadgeTone::Info => palette::link_blue(),
        };
        row = row.child(
            text(badge.clone())
                .w(px(36.0))
                .app_class("font-mono")
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color),
        );
    }

    row = row.child(
        text(line.text.clone())
            .app_class("font-mono")
            .text_size(px(12.0))
            .text_color(palette::text_primary()),
    );

    if let Some(suffix) = &line.muted_suffix {
        row = row.child(
            text(suffix.clone())
                .app_class("font-mono")
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        );
    }

    row
}

// --- Diff Tab ---------------------------------------------------------------------------

fn diff_tab_view(run: &WorkflowRunViewData) -> impl IntoElement {
    let total_add: usize = run.diffs.iter().map(|d| d.additions).sum();
    let total_del: usize = run.diffs.iter().map(|d| d.deletions).sum();

    let mut list = view().gap(px(12.0)).child(
        view()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                text(format!(
                    "{} files changed in worktree {}",
                    run.diffs.len(),
                    run.worktree_name
                ))
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        text(format!("+{total_add}"))
                            .app_class("font-mono")
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::diff_add_text()),
                    )
                    .child(
                        text(format!("−{total_del}"))
                            .app_class("font-mono")
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::diff_del_text()),
                    ),
            ),
    );

    if run.diffs.is_empty() {
        list = list.child(
            view()
                .p(px(20.0))
                .rounded(px(9.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_bg())
                .items_center()
                .child(
                    text("No file changes recorded for this run yet.")
                        .text_size(px(12.5))
                        .text_color(palette::text_muted()),
                ),
        );
    } else {
        for file in &run.diffs {
            list = list.child(diff_file_card(file));
        }
    }

    list
}

fn diff_file_card(file: &DiffFileChange) -> impl IntoElement {
    let mut lines_col = view().py(px(6.0));
    lines_col = lines_col.child(
        text(file.hunk_header.clone())
            .px(px(12.0))
            .py(px(3.0))
            .app_class("font-mono")
            .text_size(px(11.0))
            .text_color(palette::text_muted()),
    );

    for line in &file.lines {
        let (bg, fg) = match line.kind {
            DiffLineKind::Context => (AdaptiveColor::TRANSPARENT, palette::text_secondary()),
            DiffLineKind::Added => (palette::diff_add_bg(), palette::diff_add_text()),
            DiffLineKind::Removed => (palette::diff_del_bg(), palette::diff_del_text()),
        };
        lines_col = lines_col.child(
            view().px(px(12.0)).py(px(2.5)).bg(bg).child(
                text(line.text.clone())
                    .app_class("font-mono")
                    .text_size(px(11.5))
                    .text_color(fg),
            ),
        );
    }

    view()
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(12.0))
                .py(px(8.5))
                .bg(palette::card_inset_bg())
                .border_b_1()
                .border_color(palette::divider_color())
                .child(
                    text(file.path.clone())
                        .app_class("font-mono")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text(format!("+{}", file.additions))
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::diff_add_text()),
                        )
                        .child(
                            text(format!("−{}", file.deletions))
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::diff_del_text()),
                        ),
                ),
        )
        .child(lines_col)
}

// --- Coverage Extension Tab -------------------------------------------------------------

fn coverage_tab_view(run: &WorkflowRunViewData) -> impl IntoElement {
    let mut table = view()
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(14.0))
                .py(px(10.0))
                .bg(palette::card_inset_bg())
                .border_b_1()
                .border_color(palette::divider_color())
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(7.0))
                        .child(icons::extension_puzzle(13.0, palette::text_secondary()))
                        .child(
                            text("Test Coverage Report")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(extension_pill()),
                )
                .child(
                    text(if run.coverage.is_empty() {
                        "No coverage data yet"
                    } else {
                        "Overall 93.4% statements · +0.6% vs main"
                    })
                    .text_size(px(11.5))
                    .tabular_nums()
                    .text_color(if run.coverage.is_empty() {
                        palette::text_muted()
                    } else {
                        palette::success_text()
                    }),
                ),
        );

    if run.coverage.is_empty() {
        table = table.child(
            view()
                .p(px(20.0))
                .border_t_1()
                .border_color(palette::divider_color())
                .items_center()
                .child(
                    text("No coverage report recorded for this run yet.")
                        .text_size(px(12.5))
                        .text_color(palette::text_muted()),
                ),
        );
    } else {
        for row in &run.coverage {
            table = table.child(coverage_row(row));
        }
    }

    table
}

fn coverage_row(row: &CoverageFileRow) -> impl IntoElement {
    let status_color = if row.status == "PASS" {
        palette::success_text()
    } else {
        palette::link_blue()
    };

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(14.0))
        .py(px(9.0))
        .border_t_1()
        .border_color(palette::divider_color())
        .child(
            text(row.path.clone())
                .flex_1()
                .app_class("font-mono")
                .text_size(px(12.0))
                .text_color(palette::text_primary()),
        )
        .child(
            text(format!("{} tests", row.tests_count))
                .w(px(66.0))
                .text_size(px(11.5))
                .tabular_nums()
                .text_color(palette::text_muted()),
        )
        .child(
            text(format!(
                "Stmts {}",
                WorkflowRunViewData::format_tenths_pct(row.statements_tenths_pct)
            ))
            .w(px(86.0))
            .text_size(px(11.5))
            .tabular_nums()
            .text_color(palette::text_secondary()),
        )
        .child(
            text(format!(
                "Br {}",
                WorkflowRunViewData::format_tenths_pct(row.branches_tenths_pct)
            ))
            .w(px(72.0))
            .text_size(px(11.5))
            .tabular_nums()
            .text_color(palette::text_muted()),
        )
        .child(
            text(row.status.clone())
                .w(px(42.0))
                .app_class("font-mono")
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(status_color),
        )
}

// --- Docked Steering Composer -----------------------------------------------------------

fn steering_composer_bar(steer_draft: Signal<String>) -> impl IntoElement {
    let submit = move || {
        let draft = steer_draft.get_untracked();
        if store::steer_workflow_run_step(&draft) {
            steer_draft.set(String::new());
        }
    };

    view().px(px(22.0)).pt(px(8.0)).pb(px(14.0)).child(
        view()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .pl(px(12.0))
            .pr(px(6.0))
            .py(px(5.0))
            .rounded(px(10.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::card_bg())
            .child(
                input()
                    .test_id("wf-run-steer-input")
                    .value(move || steer_draft.get())
                    .on_change(move |value: &str| steer_draft.set(value.to_owned()))
                    .on_submit(move |_: &str| submit())
                    .placeholder("Steer this step — e.g. \"skip the e2e suite\"")
                    .flex_1()
                    .py(px(5.0))
                    .text_color(palette::text_primary()),
            )
            .child(
                button()
                    .test_id("wf-run-steer-send")
                    .accessibility_label("Send")
                    .disabled(move || {
                        steer_draft.with(|text| text.trim().is_empty())
                            || store::active_workflow_run().execution_state.is_terminal()
                    })
                    .on_press(submit)
                    .px(px(14.0))
                    .py(px(6.5))
                    .rounded(px(7.0))
                    .bg(palette::nav_active_bg())
                    .active(|s| s.opacity(0.85))
                    .disabled_style(|s| s.opacity(0.5))
                    .child(
                        text("Send")
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::nav_active_fg()),
                    ),
            ),
    )
}

// --- Right Step Inspector ---------------------------------------------------------------

fn step_inspector_panel() -> impl IntoElement {
    view()
        .w(px(INSPECTOR_WIDTH))
        .flex_shrink_0()
        .border_l_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .child(scroll().flex_1().child(dynamic(move || {
            let run = store::active_workflow_run();
            let selected_idx =
                store::workflow_run_selected_step().min(run.steps.len().saturating_sub(1));
            let Some(step) = run.steps.get(selected_idx) else {
                return view().into_element();
            };

            let mut panel = view()
                .p(px(16.0))
                .gap(px(14.0))
                // Step heading
                .child(
                    view()
                        .gap(px(3.0))
                        .child(
                            text(format!("Step {}", step.number))
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                        )
                        .child(
                            text(step.title.clone())
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        ),
                )
                // Agent section
                .child(inspector_agent_section(step))
                // Git section
                .child(inspector_git_section(step))
                // Progress section
                .child(inspector_progress_section(step))
                // Inputs section
                .child(inspector_inputs_section(step))
                // Outputs section
                .child(inspector_outputs_section(step))
                // Budget Guard extension card
                .child(budget_guard_card(&run));

            if let Some((callout_title, callout_body)) = &step.next_callout {
                panel = panel.child(next_callout_card(callout_title, callout_body));
            }

            panel.into_element()
        })))
}

fn inspector_agent_section(step: &RunStepDetail) -> impl IntoElement {
    let agent_value = match &step.agent.agent_scope {
        Some(scope) => format!("{} · {}", step.agent.agent_name, scope),
        None => step.agent.agent_name.clone(),
    };

    view()
        .gap(px(7.0))
        .pb(px(12.0))
        .border_b_1()
        .border_color(palette::divider_color())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .pb(px(2.0))
                .child(icons::user_agent(palette::text_muted()))
                .child(
                    text("Agent")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(inspector_kv_row("Agent", agent_value, false))
        .child(inspector_kv_row(
            "Provider",
            step.agent.provider.clone(),
            false,
        ))
        .child(inspector_kv_row("Model", step.agent.model.clone(), false))
        .child(inspector_kv_row(
            "Sandbox",
            step.agent.sandbox.clone(),
            false,
        ))
        .child(inspector_kv_row("Config", step.agent.config.clone(), false))
        .child(inspector_kv_row(
            "Approvals",
            step.agent.approvals.clone(),
            false,
        ))
}

fn inspector_git_section(step: &RunStepDetail) -> impl IntoElement {
    view()
        .gap(px(7.0))
        .pb(px(12.0))
        .border_b_1()
        .border_color(palette::divider_color())
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .pb(px(2.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(icons::git_branch(palette::text_muted()))
                        .child(
                            text("Git")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_secondary()),
                        ),
                )
                .child(
                    view()
                        .px(px(7.5))
                        .py(px(1.5))
                        .rounded(px(5.0))
                        .bg(palette::status_running_bg())
                        .child(
                            text(step.git.mode_badge.clone())
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::link_blue()),
                        ),
                ),
        )
        .child(inspector_kv_row("Machine", step.git.machine.clone(), false))
        .child(inspector_kv_row("Branch", step.git.branch.clone(), true))
        .child(inspector_kv_row(
            "Checkout",
            step.git.checkout.clone(),
            false,
        ))
        .child(inspector_kv_row(
            "Working dir",
            step.git.working_dir.clone(),
            true,
        ))
}

fn inspector_progress_section(step: &RunStepDetail) -> impl IntoElement {
    let mut col = view()
        .gap(px(7.0))
        .pb(px(12.0))
        .border_b_1()
        .border_color(palette::divider_color())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .pb(px(2.0))
                .child(icons::clock_progress(13.5, palette::text_muted()))
                .child(
                    text("Progress")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        );

    if let Some(loop_label) = &step.progress.loop_label {
        col = col.child(inspector_kv_row("Loop", loop_label.clone(), false));
    }
    if let Some(exit_when) = &step.progress.exit_when {
        col = col.child(inspector_kv_row("Exit when", exit_when.clone(), false));
    }

    col.child(inspector_kv_row(
        "Elapsed",
        step.progress.elapsed.clone(),
        false,
    ))
    .child(inspector_kv_row(
        "Tokens",
        step.progress.tokens.clone(),
        false,
    ))
}

fn inspector_inputs_section(step: &RunStepDetail) -> impl IntoElement {
    let mut section = view().gap(px(5.0)).child(
        text("Inputs")
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::text_muted()),
    );

    for binding in &step.inputs {
        section = section.child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .px(px(9.0))
                .py(px(5.5))
                .rounded(px(6.5))
                .bg(palette::subtle_surface_bg())
                .child(
                    text(binding.name.clone())
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(binding.source.clone())
                        .text_size(px(11.0))
                        .text_color(palette::text_muted()),
                ),
        );
    }

    section
}

fn inspector_outputs_section(step: &RunStepDetail) -> impl IntoElement {
    let mut chips = view().flex_row().flex_wrap().gap(px(6.0));
    for output in &step.outputs {
        let label = if output.pending {
            format!("{} · pending", output.name)
        } else {
            output.name.clone()
        };
        chips = chips.child(
            view()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .child(
                    text(label)
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .text_color(if output.pending {
                            palette::text_muted()
                        } else {
                            palette::text_primary()
                        }),
                ),
        );
    }

    view()
        .gap(px(6.0))
        .child(
            text("Outputs")
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
        .child(chips)
}

fn budget_guard_card(run: &WorkflowRunViewData) -> impl IntoElement {
    let guard = &run.budget_guard;
    let fill_fraction = guard.ratio().clamp(0.04, 1.0);
    let checked = guard.pause_when_reached;

    view()
        .p(px(12.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .gap(px(9.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(5.5))
                        .child(icons::extension_puzzle(12.5, palette::text_secondary()))
                        .child(
                            text("Budget Guard")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    text(format!(
                        "{} / {}",
                        guard.formatted_spent(),
                        guard.formatted_limit()
                    ))
                    .text_size(px(11.5))
                    .tabular_nums()
                    .text_color(palette::text_muted()),
                ),
        )
        .child(
            view()
                .h(px(4.5))
                .w_full()
                .rounded_full()
                .bg(palette::segmented_track_bg())
                .overflow_hidden()
                .child(
                    view()
                        .h_full()
                        .w(relative(fill_fraction))
                        .rounded_full()
                        .bg(palette::nav_active_bg()),
                ),
        )
        .child(
            button()
                .test_id("wf-run-budget-pause")
                .accessibility_label("Pause the run if the budget is reached")
                .on_press(store::toggle_workflow_run_budget_pause)
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .active(|s| s.opacity(0.8))
                .child(
                    view()
                        .w(px(15.0))
                        .h(px(15.0))
                        .flex_shrink_0()
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(if checked {
                            palette::nav_active_bg()
                        } else {
                            palette::border_color()
                        })
                        .bg(if checked {
                            palette::nav_active_bg()
                        } else {
                            palette::control_bg()
                        })
                        .items_center()
                        .justify_center()
                        .child(if checked {
                            icons::check(palette::nav_active_fg()).into_element()
                        } else {
                            view().into_element()
                        }),
                )
                .child(
                    text("Pause the run if the budget is reached")
                        .flex_1()
                        .text_size(px(11.5))
                        .text_color(palette::text_secondary()),
                ),
        )
}

fn next_callout_card(title: &str, body: &str) -> impl IntoElement {
    view()
        .p(px(12.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::callout_amber_border())
        .bg(palette::callout_amber_bg())
        .gap(px(4.0))
        .child(
            text(title.to_owned())
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::attempt_banner_text()),
        )
        .child(
            text(body.to_owned())
                .text_size(px(11.5))
                .text_color(palette::text_secondary()),
        )
}

fn inspector_kv_row(label: &'static str, value: String, monospace: bool) -> impl IntoElement {
    let mut val_el = text(value)
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .tabular_nums()
        .text_color(palette::text_primary());
    if monospace {
        val_el = val_el.app_class("font-mono").text_size(px(11.5));
    }

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            text(label)
                .flex_shrink_0()
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(val_el)
}

fn extension_pill() -> impl IntoElement {
    view()
        .px(px(6.0))
        .py(px(1.5))
        .rounded(px(4.5))
        .bg(palette::subtle_surface_bg())
        .border_1()
        .border_color(palette::divider_color())
        .child(
            text("Extension")
                .text_size(px(10.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
}
