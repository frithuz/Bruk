//! Project Work Queue view: intake banner, Board/List modes, unmatched issues drawer,
//! and right-hand issue inspector matching `04 WorkQueue.png`.

use factory_core::{
    ProjectSettingsTab, ProjectWorkQueue, UnmatchedIssue, WorkQueueItem, WorkQueueStage,
    WorkQueueViewMode,
};
use nsh_base::{button, input};
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{icons, palette},
};

const INSPECTOR_WIDTH: f32 = 294.0;

/// Main Work Queue split view (board/list canvas on left, contextual inspector on right).
pub(crate) fn work_queue_view() -> impl IntoElement {
    let workflow_picker_open = signal(false);
    let ad_hoc_workflow = signal("Release prep".to_owned());
    let ad_hoc_title = signal(String::new());

    view()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .flex_row()
        .bg(palette::canvas_bg())
        .child(scroll().flex_1().min_w(px(0.0)).child(dynamic(move || {
            if store::selected_project().is_none() {
                return view()
                    .test_id("wq-no-project-empty")
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .py(px(72.0))
                    .px(px(24.0))
                    .gap(px(10.0))
                    .child(
                        text("No project selected")
                            .text_size(px(17.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                    )
                    .child(
                        text("Register a local repository to manage its issue intake and work queue.")
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
                    .into_element();
            }

            let queue = store::active_work_queue();
            let view_mode = store::work_queue_view_mode();
            let selected_issue = store::selected_work_queue_issue()
                .filter(|num| queue.item(*num).is_some())
                .or_else(|| queue.default_selected_issue());
            let ad_hoc_open = store::work_queue_ad_hoc_open();
            let unmatched_expanded = store::work_queue_unmatched_expanded();

            let mut body = view()
                .flex_1()
                .p(px(18.0))
                .gap(px(14.0))
                .child(intake_banner(&queue));

            if ad_hoc_open {
                body = body.child(ad_hoc_run_card(&queue, ad_hoc_workflow, ad_hoc_title));
            }

            let is_unconfigured_empty = queue.items.is_empty()
                && queue.unmatched_issues.is_empty()
                && queue.repo_slug.is_empty();

            if is_unconfigured_empty {
                body = body.child(work_queue_empty_card());
            } else {
                body = match view_mode {
                    WorkQueueViewMode::Board => {
                        body.child(board_view(&queue, selected_issue, workflow_picker_open))
                    }
                    WorkQueueViewMode::List => {
                        body.child(list_view(&queue, selected_issue, workflow_picker_open))
                    }
                };
            }

            if !queue.unmatched_issues.is_empty() {
                body = body.child(unmatched_issues_footer(&queue, unmatched_expanded));
            }
            body.into_element()
        })))
        .child(when(store::work_queue_inspector_visible, move || {
            inspector_panel(workflow_picker_open)
        }))
}

// --- Intake Banner & Ad-Hoc Run Card ----------------------------------------------------

fn intake_banner(queue: &ProjectWorkQueue) -> impl IntoElement {
    let summary_text = queue.intake_summary_sentence();

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(14.0))
        .px(px(14.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .bg(palette::intake_banner_bg())
        .border_1()
        .border_color(palette::intake_banner_border())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(icons::funnel(palette::link_blue()))
                .child(
                    text(summary_text)
                        .flex_1()
                        .text_size(px(12.5))
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .test_id("wq-edit-intake-rules")
                .accessibility_label("Edit intake rules")
                .on_press(|| {
                    store::open_project_settings_tab(ProjectSettingsTab::IssuesAndIntake)
                })
                .flex_shrink_0()
                .hover(|s| s.opacity(0.8))
                .active(|s| s.opacity(0.65))
                .child(
                    view()
                        .border_b_1()
                        .border_color(palette::link_blue())
                        .child(
                            text("Edit intake rules")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::link_blue()),
                        ),
                ),
        )
}

fn ad_hoc_run_card(
    queue: &ProjectWorkQueue,
    ad_hoc_workflow: Signal<String>,
    ad_hoc_title: Signal<String>,
) -> impl IntoElement {
    let mut workflow_pills = view().flex_row().items_center().gap(px(6.0));
    if queue.workflows.is_empty() {
        workflow_pills = workflow_pills.child(
            text("No workflows configured for this project yet — running with default ad-hoc workflow.")
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        );
    } else {
        for wf in &queue.workflows {
            let wf_name = wf.name.clone();
            let wf_border = wf_name.clone();
            let wf_bg = wf_name.clone();
            let wf_text = wf_name.clone();
            let wf_press = wf_name.clone();
            workflow_pills = workflow_pills.child(
                button()
                    .on_press(move || ad_hoc_workflow.set(wf_press.clone()))
                    .px(px(9.0))
                    .py(px(4.5))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(move || {
                        if ad_hoc_workflow.get() == wf_border {
                            palette::nav_active_bg()
                        } else {
                            palette::border_color()
                        }
                    })
                    .bg(move || {
                        if ad_hoc_workflow.get() == wf_bg {
                            palette::nav_active_bg()
                        } else {
                            palette::control_bg()
                        }
                    })
                    .child(
                        text(wf_name)
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(move || {
                                if ad_hoc_workflow.get() == wf_text {
                                    palette::nav_active_fg()
                                } else {
                                    palette::text_primary()
                                }
                            }),
                    ),
            );
        }
    }

    let launch = move || {
        let wf = ad_hoc_workflow.get_untracked();
        let title = ad_hoc_title.get_untracked();
        if store::launch_ad_hoc_work_run(&wf, &title) {
            ad_hoc_title.set(String::new());
        }
    };

    view()
        .p(px(14.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("Start an ad-hoc run")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .on_press(|| store::set_work_queue_ad_hoc_open(false))
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(px(6.0))
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Close")
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(workflow_pills)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    input()
                        .test_id("wq-adhoc-title")
                        .value(move || ad_hoc_title.get())
                        .on_change(move |v: &str| ad_hoc_title.set(v.to_owned()))
                        .on_submit(move |_: &str| launch())
                        .placeholder("Describe the ad-hoc task to run...")
                        .flex_1()
                        .px(px(10.0))
                        .py(px(7.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .text_size(px(12.5))
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .disabled(move || ad_hoc_title.with(|t| t.trim().is_empty()))
                        .on_press(launch)
                        .px(px(13.0))
                        .py(px(7.0))
                        .rounded(px(7.0))
                        .bg(palette::nav_active_bg())
                        .active(|s| s.opacity(0.8))
                        .disabled_style(|s| s.opacity(0.4))
                        .child(
                            text("Start Ad-hoc Run")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        )
}

// --- Empty State & Board View (3-column Kanban matching 04 WorkQueue.png) ---------------

fn work_queue_empty_card() -> impl IntoElement {
    view()
        .test_id("wq-empty-state")
        .p(px(36.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .child(
            view()
                .w(px(40.0))
                .h(px(40.0))
                .rounded(px(10.0))
                .bg(palette::subtle_surface_bg())
                .border_1()
                .border_color(palette::divider_color())
                .items_center()
                .justify_center()
                .child(icons::work_queue(palette::text_secondary())),
        )
        .child(
            text("Work queue is empty")
                .text_size(px(16.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            text(
                "Connect GitHub Issues or itsaplan in Project Settings to automatically pull labeled issues, or launch an ad-hoc run.",
            )
            .text_center()
            .w(px(440.0))
            .text_size(px(12.5))
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
                        .test_id("wq-empty-configure-source")
                        .accessibility_label("Configure Issue Source")
                        .on_press(|| {
                            store::open_project_settings_tab(ProjectSettingsTab::IssuesAndIntake)
                        })
                        .h(px(32.0))
                        .px(px(14.0))
                        .rounded(px(8.0))
                        .bg(palette::nav_active_bg())
                        .items_center()
                        .justify_center()
                        .active(|s| s.opacity(0.85))
                        .child(
                            text("Configure Issue Source…")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                )
                .child(
                    button()
                        .on_press(|| store::set_work_queue_ad_hoc_open(true))
                        .h(px(32.0))
                        .px(px(12.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Start Ad-hoc Run…")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
}

fn board_view(
    queue: &ProjectWorkQueue,
    selected_issue: Option<u32>,
    workflow_picker_open: Signal<bool>,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_start()
        .gap(px(14.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(14.0))
                .child(lane_container(
                    queue,
                    WorkQueueStage::ReadyForAgents,
                    selected_issue,
                    workflow_picker_open,
                ))
                .child(lane_container(
                    queue,
                    WorkQueueStage::DoneThisWeek,
                    selected_issue,
                    workflow_picker_open,
                )),
        )
        .child(view().flex_1().min_w(px(0.0)).child(lane_container(
            queue,
            WorkQueueStage::InProgress,
            selected_issue,
            workflow_picker_open,
        )))
        .child(view().flex_1().min_w(px(0.0)).child(lane_container(
            queue,
            WorkQueueStage::NeedsYou,
            selected_issue,
            workflow_picker_open,
        )))
}

fn lane_container(
    queue: &ProjectWorkQueue,
    stage: WorkQueueStage,
    selected_issue: Option<u32>,
    workflow_picker_open: Signal<bool>,
) -> impl IntoElement {
    let items = queue.items_in_stage(stage);
    let count = items.len();

    let mut lane = view()
        .p(px(10.0))
        .rounded(px(12.0))
        .bg(palette::lane_bg())
        .gap(px(9.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .px(px(2.0))
                .child(
                    text(stage.label())
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .min_w(px(20.0))
                        .px(px(6.5))
                        .py(px(1.5))
                        .rounded_full()
                        .items_center()
                        .justify_center()
                        .bg(palette::queue_badge_bg())
                        .child(
                            text(count.to_string())
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .tabular_nums()
                                .text_color(palette::text_muted()),
                        ),
                ),
        );

    if items.is_empty() {
        lane = lane.child(
            view()
                .px(px(10.0))
                .py(px(14.0))
                .rounded(px(9.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_bg())
                .child(
                    text("No issues in this column")
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        );
    } else {
        for item in items {
            let is_selected = selected_issue == Some(item.issue_number);
            lane = lane.child(issue_card(item, is_selected, workflow_picker_open));
        }
    }

    lane
}

fn issue_card(
    item: &WorkQueueItem,
    is_selected: bool,
    workflow_picker_open: Signal<bool>,
) -> impl IntoElement {
    let issue_number = item.issue_number;
    let stage = item.stage;
    let is_next_in_queue =
        stage == WorkQueueStage::ReadyForAgents && item.status_text == "Next in queue";

    let workflow_line = match item.run_number {
        Some(run_no) => format!("{} · run #{run_no}", item.workflow_name),
        None => item.workflow_name.clone(),
    };

    let card_bg = if is_selected {
        palette::card_selected_bg()
    } else {
        palette::card_bg()
    };
    let card_border = if is_selected {
        palette::card_selected_border()
    } else {
        palette::border_color()
    };

    let mut card = view()
        .p(px(11.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(card_border)
        .bg(card_bg)
        .gap(px(8.0));

    if is_selected {
        card = card.border_2();
    }

    // Upper clickable region selects this issue in the right inspector.
    let card_test_id = format!("wq-card-{issue_number}");
    let select_body = button()
        .test_id(&card_test_id)
        .accessibility_label(format!("#{issue_number} {}", item.title))
        .on_press(move || {
            workflow_picker_open.set(false);
            store::select_work_queue_issue(issue_number);
        })
        .gap(px(6.0))
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
                        .gap(px(5.0))
                        .child(icons::issue_open(palette::text_muted()))
                        .child(
                            text(format!("#{issue_number}"))
                                .text_size(px(11.5))
                                .tabular_nums()
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(label_pill(&item.label)),
        )
        .child(
            text(item.title.clone())
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .child(icons::workflow_branch(12.5, palette::text_muted()))
                .child(
                    text(workflow_line)
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        );

    let mut footer = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .pt(px(2.0))
        .child(
            button()
                .on_press(move || {
                    workflow_picker_open.set(false);
                    store::select_work_queue_issue(issue_number);
                })
                .flex_1()
                .min_w(px(0.0))
                .child(status_indicator(stage, &item.status_text)),
        );

    if is_next_in_queue {
        let start_id = format!("wq-card-start-{issue_number}");
        footer = footer.child(
            button()
                .test_id(&start_id)
                .accessibility_label("Start Now")
                .on_press(move || store::start_work_queue_issue(issue_number))
                .px(px(9.5))
                .py(px(4.0))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Start Now")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                ),
        );
    } else if stage == WorkQueueStage::NeedsYou {
        let review_id = format!("wq-card-review-{issue_number}");
        footer = footer.child(
            button()
                .test_id(&review_id)
                .accessibility_label("Review")
                .on_press(move || {
                    workflow_picker_open.set(false);
                    store::select_work_queue_issue(issue_number);
                })
                .px(px(9.5))
                .py(px(4.0))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Review")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                ),
        );
    }

    card.child(select_body).child(footer)
}

fn label_pill(label: &str) -> impl IntoElement {
    view()
        .px(px(7.5))
        .py(px(1.5))
        .rounded_full()
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::tag_pill_bg())
        .child(
            text(label.to_owned())
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
}

fn status_indicator(stage: WorkQueueStage, status_text: &str) -> impl IntoElement {
    let status_owned = status_text.to_owned();
    let (icon, color, weight) = match stage {
        WorkQueueStage::ReadyForAgents => (
            icons::status_queued(palette::text_muted()).into_element(),
            palette::text_muted(),
            FontWeight::NORMAL,
        ),
        WorkQueueStage::InProgress => (
            icons::status_running(palette::link_blue()).into_element(),
            palette::link_blue(),
            FontWeight::MEDIUM,
        ),
        WorkQueueStage::NeedsYou => (
            icons::status_review(palette::review_amber()).into_element(),
            palette::review_amber(),
            FontWeight::MEDIUM,
        ),
        WorkQueueStage::DoneThisWeek => (
            icons::status_merged(palette::success_text()).into_element(),
            palette::success_text(),
            FontWeight::MEDIUM,
        ),
    };

    view()
        .flex_row()
        .items_center()
        .gap(px(5.5))
        .child(icon)
        .child(
            text(status_owned)
                .text_size(px(11.5))
                .font_weight(weight)
                .text_color(color),
        )
}

// --- List View --------------------------------------------------------------------------

fn list_view(
    queue: &ProjectWorkQueue,
    selected_issue: Option<u32>,
    workflow_picker_open: Signal<bool>,
) -> impl IntoElement {
    let mut stack = view().gap(px(12.0));

    for stage in WorkQueueStage::ALL {
        let items = queue.items_in_stage(*stage);
        let mut group = view()
            .rounded(px(12.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::card_bg())
            .overflow_hidden()
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(7.0))
                    .px(px(14.0))
                    .py(px(9.0))
                    .bg(palette::lane_bg())
                    .border_b_1()
                    .border_color(palette::divider_color())
                    .child(
                        text(stage.label())
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                    )
                    .child(
                        view()
                            .min_w(px(20.0))
                            .px(px(6.5))
                            .py(px(1.0))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(palette::queue_badge_bg())
                            .child(
                                text(items.len().to_string())
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .tabular_nums()
                                    .text_color(palette::text_muted()),
                            ),
                    ),
            );

        if items.is_empty() {
            group = group.child(
                view().px(px(14.0)).py(px(12.0)).child(
                    text("No issues in this stage")
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
            );
        } else {
            let total = items.len();
            for (idx, item) in items.into_iter().enumerate() {
                let is_selected = selected_issue == Some(item.issue_number);
                let is_last = idx + 1 == total;
                group = group.child(list_issue_row(
                    item,
                    is_selected,
                    is_last,
                    workflow_picker_open,
                ));
            }
        }

        stack = stack.child(group);
    }

    stack
}

fn list_issue_row(
    item: &WorkQueueItem,
    is_selected: bool,
    is_last: bool,
    workflow_picker_open: Signal<bool>,
) -> impl IntoElement {
    let issue_number = item.issue_number;
    let stage = item.stage;
    let workflow_line = match item.run_number {
        Some(run_no) => format!("{} · run #{run_no}", item.workflow_name),
        None => item.workflow_name.clone(),
    };

    let mut row = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(14.0))
        .py(px(10.0))
        .bg(if is_selected {
            palette::card_selected_bg()
        } else {
            palette::card_bg()
        });

    if !is_last {
        row = row.border_b_1().border_color(palette::divider_color());
    }

    let list_row_id = format!("wq-list-row-{issue_number}");
    row = row
        .child(
            button()
                .test_id(&list_row_id)
                .accessibility_label(format!("#{issue_number} {}", item.title))
                .on_press(move || {
                    workflow_picker_open.set(false);
                    store::select_work_queue_issue(issue_number);
                })
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(icons::issue_open(palette::text_muted()))
                .child(
                    text(format!("#{issue_number}"))
                        .w(px(40.0))
                        .text_size(px(12.0))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(item.title.clone())
                        .flex_1()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(label_pill(&item.label))
                .child(
                    view()
                        .w(px(170.0))
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .child(icons::workflow_branch(12.0, palette::text_muted()))
                        .child(
                            text(workflow_line)
                                .text_size(px(12.0))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    view()
                        .w(px(180.0))
                        .child(status_indicator(stage, &item.status_text)),
                ),
        )
        .child(match stage {
            WorkQueueStage::ReadyForAgents => button()
                .on_press(move || store::start_work_queue_issue(issue_number))
                .px(px(9.0))
                .py(px(4.0))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text("Start Now")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .into_element(),
            WorkQueueStage::NeedsYou => button()
                .on_press(move || {
                    workflow_picker_open.set(false);
                    store::select_work_queue_issue(issue_number);
                })
                .px(px(9.0))
                .py(px(4.0))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text("Review")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .into_element(),
            _ => view().w(px(68.0)).into_element(),
        });

    row
}

// --- Unmatched Issues Footer Disclosure -------------------------------------------------

fn unmatched_issues_footer(queue: &ProjectWorkQueue, expanded: bool) -> impl IntoElement {
    let count = queue.unmatched_issues.len();
    let disclosure_label = if count == 1 {
        "1 open issue doesn't match the intake rules".to_owned()
    } else {
        format!("{count} open issues don't match the intake rules")
    };

    let mut section = view().pt(px(2.0)).gap(px(10.0)).child(
        button()
            .test_id("wq-unmatched-toggle")
            .accessibility_label(disclosure_label.clone())
            .on_press(store::toggle_work_queue_unmatched_expanded)
            .flex_row()
            .items_center()
            .gap(px(5.0))
            .hover(|s| s.opacity(0.8))
            .child(if expanded {
                icons::chevron_down(palette::link_blue()).into_element()
            } else {
                icons::chevron_right(palette::link_blue()).into_element()
            })
            .child(
                text(disclosure_label)
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::link_blue()),
            ),
    );

    if expanded && !queue.unmatched_issues.is_empty() {
        let mut card = view()
            .rounded(px(10.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::card_bg())
            .overflow_hidden();

        let total = queue.unmatched_issues.len();
        for (idx, issue) in queue.unmatched_issues.iter().enumerate() {
            card = card.child(unmatched_issue_row(issue, idx + 1 == total));
        }
        section = section.child(card);
    }

    section
}

fn unmatched_issue_row(issue: &UnmatchedIssue, is_last: bool) -> impl IntoElement {
    let issue_number = issue.issue_number;
    let mut row = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(12.0))
        .py(px(8.5));

    if !is_last {
        row = row.border_b_1().border_color(palette::divider_color());
    }

    let mut labels_row = view().flex_row().items_center().gap(px(5.0));
    for label in &issue.labels {
        labels_row = labels_row.child(label_pill(label));
    }

    row.child(
        view()
            .flex_1()
            .min_w(px(0.0))
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(
                text(format!("#{issue_number}"))
                    .w(px(38.0))
                    .text_size(px(11.5))
                    .tabular_nums()
                    .text_color(palette::text_muted()),
            )
            .child(
                text(issue.title.clone())
                    .flex_1()
                    .text_size(px(12.5))
                    .text_color(palette::text_primary()),
            )
            .child(labels_row)
            .child(
                text(issue.reason.clone())
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
            ),
    )
    .child({
        let queue_btn_id = format!("wq-queue-unmatched-{issue_number}");
        button()
            .test_id(&queue_btn_id)
            .accessibility_label(format!("Queue #{issue_number}"))
            .on_press(move || store::queue_unmatched_work_issue(issue_number))
            .px(px(9.0))
            .py(px(3.5))
            .rounded(px(6.5))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::control_bg())
            .hover(|s| s.bg(palette::subtle_surface_bg()))
            .child(
                text("Queue")
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_primary()),
            )
    })
}

// --- Right Inspector Panel --------------------------------------------------------------

fn inspector_panel(workflow_picker_open: Signal<bool>) -> impl IntoElement {
    view()
        .w(px(INSPECTOR_WIDTH))
        .flex_shrink_0()
        .border_l_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .child(scroll().flex_1().child(dynamic(move || {
            let queue = store::active_work_queue();
            let selected_issue = store::selected_work_queue_issue()
                .filter(|num| queue.item(*num).is_some())
                .or_else(|| queue.default_selected_issue());
            let picker_open = workflow_picker_open.get();

            let Some(item) = selected_issue.and_then(|num| queue.item(num)) else {
                return view()
                    .p(px(18.0))
                    .gap(px(6.0))
                    .child(
                        text("No issue selected")
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                    )
                    .child(
                        text("Select an issue card from the board to inspect its intake rule and actions.")
                            .text_size(px(12.0))
                            .text_color(palette::text_muted()),
                    )
                    .into_element();
            };

            inspector_content(&queue, item, picker_open, workflow_picker_open).into_element()
        })))
}

fn inspector_content(
    queue: &ProjectWorkQueue,
    item: &WorkQueueItem,
    picker_open: bool,
    workflow_picker_open: Signal<bool>,
) -> impl IntoElement {
    let issue_number = item.issue_number;
    let issue_url = item.issue_url.clone();
    let tracker_open_label = if queue.tracker_name.contains("Linear") {
        "Open on Linear"
    } else {
        "Open on GitHub"
    };
    let tracker_short = if queue.tracker_name.contains("Linear") {
        "Linear"
    } else {
        "GitHub"
    };

    let mut panel = view()
        .p(px(18.0))
        .gap(px(14.0))
        // Issue heading block
        .child(
            view()
                .gap(px(5.0))
                .child(
                    text(format!("{} #{issue_number}", queue.repo_slug))
                        .text_size(px(12.0))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(item.title.clone())
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view().flex_row().child(
                        button()
                            .accessibility_label(tracker_open_label)
                            .on_press(move || {
                                store::set_status_message(format!("Issue URL: {issue_url}"));
                            })
                            .hover(|s| s.opacity(0.8))
                            .child(
                                view()
                                    .border_b_1()
                                    .border_color(palette::link_blue())
                                    .child(
                                        text(tracker_open_label)
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(palette::link_blue()),
                                    ),
                            ),
                    ),
                ),
        )
        // Description
        .child(
            text(item.description.clone())
                .text_size(px(12.5))
                .text_color(palette::text_secondary()),
        );

    // Metadata key-value rows
    let mut meta = view().gap(px(7.5)).child(metadata_row(
        "Matched rule",
        item.matched_rule.clone(),
        palette::text_primary(),
    ));

    match item.stage {
        WorkQueueStage::ReadyForAgents => {
            if let Some((pos, total)) = queue.queue_position(issue_number) {
                meta = meta.child(metadata_row(
                    "Queue position",
                    format!("{pos} of {total}"),
                    palette::text_primary(),
                ));
            }
            meta = meta.child(metadata_row(
                "Starts",
                queue.starts_label().to_owned(),
                palette::text_primary(),
            ));
        }
        WorkQueueStage::InProgress => {
            let wf = match item.run_number {
                Some(run_no) => format!("{} · run #{run_no}", item.workflow_name),
                None => item.workflow_name.clone(),
            };
            meta = meta
                .child(metadata_row("Workflow", wf, palette::text_primary()))
                .child(metadata_row(
                    "Status",
                    item.status_text.clone(),
                    palette::link_blue(),
                ));
        }
        WorkQueueStage::NeedsYou => {
            let wf = match item.run_number {
                Some(run_no) => format!("{} · run #{run_no}", item.workflow_name),
                None => item.workflow_name.clone(),
            };
            meta = meta
                .child(metadata_row("Workflow", wf, palette::text_primary()))
                .child(metadata_row(
                    "Gate",
                    item.status_text.clone(),
                    palette::review_amber(),
                ));
        }
        WorkQueueStage::DoneThisWeek => {
            let wf = match item.run_number {
                Some(run_no) => format!("{} · run #{run_no}", item.workflow_name),
                None => item.workflow_name.clone(),
            };
            meta = meta
                .child(metadata_row("Workflow", wf, palette::text_primary()))
                .child(metadata_row(
                    "Outcome",
                    item.status_text.clone(),
                    palette::success_text(),
                ));
        }
    }

    panel = panel.child(meta);

    // Stage-specific action buttons
    panel = panel.child(inspector_actions(
        queue,
        item,
        picker_open,
        workflow_picker_open,
    ));

    // "Bruk will update the issue" bulleted list
    panel = panel.child(
        view()
            .pt(px(6.0))
            .gap(px(6.0))
            .child(
                text("Bruk will update the issue")
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_secondary()),
            )
            .child(bullet_item("Add label agent-working when the run starts"))
            .child(bullet_item(
                "Comment with a link to the run and the final summary",
            ))
            .child(bullet_item(&format!(
                "Link the pull request; {tracker_short} closes the issue on merge"
            ))),
    );

    panel
}

fn metadata_row(
    label: &'static str,
    value: String,
    value_color: AdaptiveColor,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
        .child(
            text(value)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .text_color(value_color),
        )
}

fn inspector_actions(
    queue: &ProjectWorkQueue,
    item: &WorkQueueItem,
    picker_open: bool,
    workflow_picker_open: Signal<bool>,
) -> impl IntoElement {
    let issue_number = item.issue_number;

    match item.stage {
        WorkQueueStage::ReadyForAgents => {
            let mut block = view()
                .gap(px(8.0))
                .child(
                    view().flex_row().child(
                        button()
                            .test_id("wq-inspector-start-now")
                            .accessibility_label("Start Now")
                            .on_press(move || {
                                workflow_picker_open.set(false);
                                store::start_work_queue_issue(issue_number);
                            })
                            .px(px(16.0))
                            .py(px(7.0))
                            .rounded(px(7.5))
                            .bg(palette::nav_active_bg())
                            .active(|s| s.opacity(0.8))
                            .child(
                                text("Start Now")
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::nav_active_fg()),
                            ),
                    ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            button()
                                .test_id("wq-inspector-use-another-workflow")
                                .accessibility_label("Use Another Workflow...")
                                .on_press(move || workflow_picker_open.set(!picker_open))
                                .px(px(11.0))
                                .py(px(6.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text("Use Another Workflow...")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("wq-inspector-skip")
                                .accessibility_label("Skip")
                                .on_press(move || {
                                    workflow_picker_open.set(false);
                                    store::skip_work_queue_issue(issue_number);
                                })
                                .px(px(11.0))
                                .py(px(6.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text("Skip")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                );

            if picker_open {
                let mut picker = view()
                    .p(px(8.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_inset_bg())
                    .gap(px(4.0))
                    .child(
                        text("Assign workflow")
                            .px(px(6.0))
                            .pb(px(2.0))
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_muted()),
                    );

                for wf in &queue.workflows {
                    let wf_name = wf.name.clone();
                    let wf_press = wf_name.clone();
                    let is_current = item.workflow_name == wf_name;
                    picker = picker.child(
                        button()
                            .on_press(move || {
                                store::reassign_work_queue_workflow(issue_number, &wf_press);
                                workflow_picker_open.set(false);
                            })
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(8.0))
                            .py(px(5.0))
                            .rounded(px(6.0))
                            .bg(if is_current {
                                palette::subtle_surface_bg()
                            } else {
                                AdaptiveColor::TRANSPARENT
                            })
                            .hover(|s| s.bg(palette::sidebar_hover_bg()))
                            .child(
                                text(wf_name)
                                    .text_size(px(12.0))
                                    .font_weight(if is_current {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(palette::text_primary()),
                            )
                            .child(if is_current {
                                icons::check(palette::link_blue()).into_element()
                            } else {
                                view().into_element()
                            }),
                    );
                }

                block = block.child(picker);
            }

            block
        }
        WorkQueueStage::InProgress => view().flex_row().gap(px(8.0)).child(
            button()
                .test_id("wq-inspector-stop")
                .on_press(move || store::stop_work_queue_issue(issue_number))
                .px(px(12.0))
                .py(px(6.5))
                .rounded(px(7.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text("Stop & Re-queue")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        ),
        WorkQueueStage::NeedsYou => view()
            .gap(px(8.0))
            .child(
                view().flex_row().child(
                    button()
                        .test_id("wq-inspector-approve")
                        .on_press(move || store::approve_work_queue_review(issue_number))
                        .px(px(14.0))
                        .py(px(7.0))
                        .rounded(px(7.5))
                        .bg(palette::nav_active_bg())
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Approve & Merge")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
            )
            .child(
                view().flex_row().child(
                    button()
                        .test_id("wq-inspector-request-changes")
                        .on_press(move || store::request_changes_work_queue_review(issue_number))
                        .px(px(11.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Request Changes")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
            ),
        WorkQueueStage::DoneThisWeek => view().flex_row().child(
            button()
                .test_id("wq-inspector-requeue")
                .on_press(move || store::requeue_work_queue_issue(issue_number))
                .px(px(12.0))
                .py(px(6.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text("Re-queue Issue")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        ),
    }
}

fn bullet_item(body: &str) -> impl IntoElement {
    view()
        .flex_row()
        .items_start()
        .gap(px(7.0))
        .child(
            text("•")
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
        .child(
            text(body.to_owned())
                .flex_1()
                .text_size(px(12.0))
                .text_color(palette::text_secondary()),
        )
}
