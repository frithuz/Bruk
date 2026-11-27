//! Project Workflows overview view: filter/sort toolbar, workflows table with 10-run outcome
//! strip, starter template cards, inline create/import drawers, and right-hand workflow inspector
//! matching `12 Workflows.png`.

use factory_workflows::{
    ProjectWorkflowsOverview, RecentRunStatus, RunOutcomeDot, WorkflowNowTone,
    WorkflowOverviewEntry, WorkflowRecentRun, WorkflowStepGroup, WorkflowStepPreview,
    WorkflowTemplateKind, WorkflowTriggerKind, WorkflowsFilter, WorkflowsSort,
};
use nsh_base::{SelectItem, SelectValue, button, input, select};
use nsh_core::prelude::*;

use factory_store as store;
use factory_ui_kit::{icons, palette};

const INSPECTOR_WIDTH: f32 = 300.0;

/// Header bar controls shown on the right when `ActiveView::Workflows` is active.
pub fn workflows_header_controls() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .window_drag(false)
        .child(
            view()
                .h(px(30.0))
                .w(px(210.0))
                .px(px(9.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .flex_row()
                .items_center()
                .gap(px(6.5))
                .child(icons::search(palette::text_muted()))
                .child(
                    input()
                        .test_id("wfo-header-filter")
                        .value(store::workflows_search)
                        .on_change(|next: &str| store::set_workflows_search(next))
                        .placeholder("Filter workflows")
                        .flex_1()
                        .min_w(px(0.0))
                        .bg(AdaptiveColor::TRANSPARENT)
                        .text_size(px(12.5))
                        .text_color(palette::text_primary()),
                ),
        )
        .child(when(
            || !store::demo_mode(),
            || {
                button()
                    .test_id("wfo-header-reload")
                    .accessibility_label("Reload .agents/bruk")
                    .on_press(store::reload_active_project_catalog)
                    .h(px(30.0))
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::control_bg())
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                    .active(|s| s.opacity(0.8))
                    .child(
                        text("Reload")
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_primary()),
                    )
            },
        ))
        .child(
            button()
                .test_id("wfo-header-import")
                .accessibility_label("Import...")
                .on_press(|| {
                    let open = store::workflows_import_drawer_open();
                    store::set_workflows_import_drawer_open(!open);
                })
                .h(px(30.0))
                .px(px(11.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Import...")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .test_id("wfo-header-new")
                .accessibility_label("New Workflow")
                .on_press(|| {
                    let open = store::workflows_new_drawer_open();
                    store::set_workflows_new_drawer_open(!open);
                })
                .h(px(30.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .bg(palette::nav_active_bg())
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .active(|s| s.opacity(0.85))
                .child(icons::plus(palette::nav_active_fg()))
                .child(
                    text("New Workflow")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        )
}

/// Main Workflows overview split canvas (table & templates on left, workflow inspector on right).
pub fn workflows_view() -> impl IntoElement {
    let new_name = signal(String::new());
    let new_desc = signal(String::new());
    let new_label = signal(String::new());
    let new_is_draft = signal(true);
    let import_path = signal("canary-deploy.bruk-workflow".to_owned());

    let edit_name = signal(String::new());
    let edit_desc = signal(String::new());
    let edit_is_draft = signal(false);
    let ad_hoc_summary = signal(String::new());

    view()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .flex_row()
        .bg(palette::canvas_bg())
        .child(scroll().flex_1().min_w(px(0.0)).child(dynamic(move || {
            let overview = store::active_workflows_overview();
            let filter = store::workflows_filter();
            let sort = store::workflows_sort();
            let search = store::workflows_search();
            let rows = overview.filtered_and_sorted(filter, sort, &search);
            let selected_id = store::selected_workflow_overview_id()
                .filter(|id| overview.workflow(id).is_some())
                .or_else(|| rows.first().map(|w| w.id.clone()))
                .or_else(|| overview.default_selected_id());
            let new_open = store::workflows_new_drawer_open();
            let import_open = store::workflows_import_drawer_open();

            let mut body = view()
                .flex_1()
                .p(px(18.0))
                .gap(px(14.0))
                .child(filter_and_sort_bar());

            if new_open {
                body = body.child(new_workflow_drawer(
                    new_name,
                    new_desc,
                    new_label,
                    new_is_draft,
                ));
            }

            if import_open {
                body = body.child(import_workflow_drawer(import_path));
            }

            let total_workflows = overview.workflows.len();
            body = body.child(workflows_table_card(
                &rows,
                total_workflows,
                selected_id.as_deref(),
            ));
            if total_workflows > 0 {
                body = body.child(run_outcome_legend());
            }
            body = body.child(templates_section());

            body
        })))
        .child(when(store::workflows_inspector_visible, move || {
            inspector_panel(edit_name, edit_desc, edit_is_draft, ad_hoc_summary)
        }))
}

// --- Filter & Sort Bar ------------------------------------------------------------------

fn filter_and_sort_bar() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(filter_segmented_control())
        .child(sort_dropdown())
}

fn filter_segmented_control() -> impl IntoElement {
    let mut track = view()
        .flex_row()
        .items_center()
        .h(px(30.0))
        .p(px(2.5))
        .rounded(px(8.0))
        .bg(palette::segmented_track_bg())
        .gap(px(2.0));

    for filter in WorkflowsFilter::ALL {
        track = track.child(filter_segment(*filter));
    }
    track
}

fn filter_segment(filter: WorkflowsFilter) -> impl IntoElement {
    let is_active = move || store::workflows_filter() == filter;

    button()
        .test_id(filter.test_id())
        .accessibility_label(filter.label())
        .on_press(move || store::set_workflows_filter(filter))
        .px(px(11.0))
        .h_full()
        .rounded(px(6.0))
        .items_center()
        .justify_center()
        .bg(move || {
            if is_active() {
                palette::control_bg()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .active(|s| s.opacity(0.8))
        .child(
            text(filter.label())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if is_active() {
                        palette::text_primary()
                    } else {
                        palette::text_muted()
                    }
                }),
        )
}

fn sort_dropdown() -> impl IntoElement {
    let trigger = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .h(px(28.0))
        .min_w(px(92.0))
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(move || store::workflows_sort().label())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()));

    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            text("Sort by")
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
        .child(
            select()
                .test_id("wfo-sort-select")
                .accessibility_label("Sort workflows")
                .items(|| {
                    WorkflowsSort::ALL
                        .iter()
                        .map(|s| SelectItem::new(s.value(), s.label()))
                        .collect::<Vec<_>>()
                })
                .value(|| SelectValue::from(store::workflows_sort().value()))
                .on_change(|value: &Option<SharedString>| {
                    if let Some(raw) = value.as_deref() {
                        store::set_workflows_sort(WorkflowsSort::from_value(raw));
                    }
                })
                .cover(false)
                .placement(Placement::Bottom)
                .align(Align::End)
                .offset(px(4.0))
                .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
                .item_class(
                    "px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium",
                )
                .trigger(trigger),
        )
}

// --- Inline Create & Import Drawers -----------------------------------------------------

fn new_workflow_drawer(
    name: Signal<String>,
    desc: Signal<String>,
    label: Signal<String>,
    is_draft: Signal<bool>,
) -> impl IntoElement {
    let submit = move || {
        let n = name.get_untracked();
        let d = desc.get_untracked();
        let l = label.get_untracked();
        let draft = is_draft.get_untracked();
        let issue_label = if l.trim().is_empty() {
            None
        } else {
            Some(l.as_str())
        };
        if store::create_overview_workflow(&n, &d, issue_label, draft) {
            name.set(String::new());
            desc.set(String::new());
            label.set(String::new());
            is_draft.set(true);
        }
    };

    view()
        .p(px(14.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::card_selected_border())
        .bg(palette::card_bg())
        .gap(px(10.0))
        .child(
            text("Create a new workflow")
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .gap(px(10.0))
                .child(labeled_input(
                    "wfo-new-name",
                    "Workflow name",
                    "e.g. Nightly smoke tests",
                    name,
                ))
                .child(labeled_input(
                    "wfo-new-desc",
                    "Short description",
                    "What this workflow automates",
                    desc,
                ))
                .child(labeled_input(
                    "wfo-new-label",
                    "Issue label (optional)",
                    "e.g. perf",
                    label,
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    button()
                        .test_id("wfo-new-draft-toggle")
                        .on_press(move || is_draft.set(!is_draft.get_untracked()))
                        .px(px(9.0))
                        .py(px(5.0))
                        .rounded(px(6.5))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::subtle_surface_bg())
                        .child(
                            text(move || {
                                if is_draft.get() {
                                    "Status: Draft"
                                } else {
                                    "Status: Published"
                                }
                            })
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_secondary()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            button()
                                .on_press(|| store::set_workflows_new_drawer_open(false))
                                .px(px(11.0))
                                .py(px(5.5))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .child(
                                    text("Cancel")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("wfo-new-submit")
                                .accessibility_label("Create Workflow")
                                .on_press(submit)
                                .px(px(12.0))
                                .py(px(5.5))
                                .rounded(px(7.0))
                                .bg(palette::nav_active_bg())
                                .active(|s| s.opacity(0.85))
                                .child(
                                    text("Create Workflow")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::nav_active_fg()),
                                ),
                        ),
                ),
        )
}

fn import_workflow_drawer(import_path: Signal<String>) -> impl IntoElement {
    let submit = move || {
        let path = import_path.get_untracked();
        if store::import_overview_workflow(&path) {
            import_path.set("canary-deploy.bruk-workflow".to_owned());
        }
    };

    view()
        .p(px(14.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::card_selected_border())
        .bg(palette::card_bg())
        .gap(px(10.0))
        .child(
            text("Import workflow from file")
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(labeled_input(
            "wfo-import-path",
            "Workflow file (.bruk-workflow)",
            "path/to/workflow.bruk-workflow",
            import_path,
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .child(
                    button()
                        .on_press(|| store::set_workflows_import_drawer_open(false))
                        .px(px(11.0))
                        .py(px(5.5))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Cancel")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    button()
                        .test_id("wfo-import-submit")
                        .accessibility_label("Import Workflow")
                        .on_press(submit)
                        .px(px(12.0))
                        .py(px(5.5))
                        .rounded(px(7.0))
                        .bg(palette::nav_active_bg())
                        .active(|s| s.opacity(0.85))
                        .child(
                            text("Import Workflow")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        )
}

fn labeled_input(
    test_id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    value: Signal<String>,
) -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(4.0))
        .child(
            text(label)
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
        .child(
            input()
                .test_id(test_id)
                .value(move || value.get())
                .on_change(move |next: &str| value.set(next.to_owned()))
                .placeholder(placeholder)
                .px(px(9.0))
                .py(px(6.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .text_size(px(12.0))
                .text_color(palette::text_primary()),
        )
}

// --- Workflows Table Card ---------------------------------------------------------------

fn workflows_table_card(
    rows: &[WorkflowOverviewEntry],
    total_workflows: usize,
    selected_id: Option<&str>,
) -> impl IntoElement {
    let mut table = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(table_header_row());

    if rows.is_empty() {
        table = table.child(empty_table_state(total_workflows == 0));
    } else {
        for (idx, workflow) in rows.iter().enumerate() {
            let is_selected = selected_id == Some(workflow.id.as_str());
            let is_last = idx + 1 == rows.len();
            table = table.child(workflow_table_row(workflow, is_selected, is_last));
        }
    }

    table
}

fn table_header_row() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(14.0))
        .py(px(9.5))
        .bg(palette::card_inset_bg())
        .border_b_1()
        .border_color(palette::border_color())
        .child(
            text("Workflow")
                .flex_1()
                .min_w(px(180.0))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
        .child(
            text("Starts from")
                .w(px(152.0))
                .flex_shrink_0()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
        .child(
            text("Last 10 runs")
                .w(px(178.0))
                .flex_shrink_0()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
        .child(
            text("Now")
                .w(px(142.0))
                .flex_shrink_0()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
}

fn empty_table_state(no_workflows_yet: bool) -> impl IntoElement {
    if no_workflows_yet {
        return view()
            .test_id("wfo-empty-state")
            .py(px(36.0))
            .px(px(20.0))
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .child(
                text("No workflows yet")
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_primary()),
            )
            .child(
                text("Create a workflow from scratch, import a .bruk-workflow file, or start from a template below.")
                    .text_center()
                    .w(px(420.0))
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
                            .test_id("wfo-empty-new")
                            .on_press(|| store::set_workflows_new_drawer_open(true))
                            .px(px(12.0))
                            .py(px(6.0))
                            .rounded(px(7.0))
                            .bg(palette::nav_active_bg())
                            .active(|s| s.opacity(0.85))
                            .child(
                                text("New Workflow")
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::nav_active_fg()),
                            ),
                    )
                    .child(
                        button()
                            .on_press(|| store::set_workflows_import_drawer_open(true))
                            .px(px(11.0))
                            .py(px(6.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::control_bg())
                            .hover(|s| s.bg(palette::subtle_surface_bg()))
                            .child(
                                text("Import...")
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_primary()),
                            ),
                    ),
            )
            .into_element();
    }

    view()
        .py(px(28.0))
        .px(px(16.0))
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            text("No workflows match the current filter")
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
        .child(
            button()
                .test_id("wfo-reset-filters")
                .on_press(store::reset_workflows_filters)
                .px(px(11.0))
                .py(px(5.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text("Reset filters")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .into_element()
}

fn workflow_table_row(
    workflow: &WorkflowOverviewEntry,
    is_selected: bool,
    is_last: bool,
) -> impl IntoElement {
    let row_id = workflow.id.clone();
    let row_test_id = format!("wfo-row-{}", workflow.id);
    let name_for_a11y = workflow.name.clone();

    let mut triggers_col = view().w(px(152.0)).flex_shrink_0().gap(px(3.5));
    for trigger in &workflow.triggers {
        let icon_el: AnyElement = match trigger.kind {
            WorkflowTriggerKind::WorkQueue => {
                icons::work_queue(palette::text_muted()).into_element()
            }
            WorkflowTriggerKind::AdHoc => icons::play_filled(palette::text_muted()).into_element(),
        };
        triggers_col = triggers_col.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(5.5))
                .child(icon_el)
                .child(
                    text(trigger.label.clone())
                        .text_size(px(11.5))
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    let mut dots_row = view().flex_row().items_center().gap(px(2.5));
    for dot in &workflow.last_ten_runs {
        dots_row = dots_row.child(run_outcome_square(*dot));
    }

    let now_color = match workflow.now_tone {
        WorkflowNowTone::Warning => palette::workflow_now_amber(),
        WorkflowNowTone::Running => palette::link_blue(),
        WorkflowNowTone::Muted => palette::text_muted(),
    };
    let now_weight = match workflow.now_tone {
        WorkflowNowTone::Warning | WorkflowNowTone::Running => FontWeight::SEMIBOLD,
        WorkflowNowTone::Muted => FontWeight::MEDIUM,
    };

    let mut title_row = view().flex_row().items_center().gap(px(7.0)).child(
        text(workflow.name.clone())
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_primary()),
    );
    if let Some(badge) = workflow.origin_badge() {
        title_row = title_row.child(
            view()
                .px(px(6.0))
                .py(px(1.0))
                .rounded(px(4.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .child(
                    text(badge)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        );
    }
    if workflow.is_draft {
        title_row = title_row.child(
            view()
                .px(px(5.5))
                .py(px(1.0))
                .rounded(px(4.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .child(
                    text("Draft")
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                ),
        );
    }

    let mut inner = view()
        .flex_row()
        .items_start()
        .gap(px(12.0))
        .px(px(14.0))
        .py(px(11.5))
        .bg(if is_selected {
            palette::card_selected_bg()
        } else {
            AdaptiveColor::TRANSPARENT
        })
        .child(
            view()
                .flex_1()
                .min_w(px(180.0))
                .gap(px(3.0))
                .child(title_row)
                .child(
                    text(workflow.description.clone())
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(triggers_col)
        .child(
            view()
                .w(px(178.0))
                .flex_shrink_0()
                .gap(px(5.0))
                .child(dots_row)
                .child(
                    text(workflow.last_run_caption.clone())
                        .text_size(px(11.0))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            view().w(px(142.0)).flex_shrink_0().pt(px(1.0)).child(
                text(workflow.now_label.clone())
                    .text_size(px(12.0))
                    .font_weight(now_weight)
                    .text_color(now_color),
            ),
        );

    if !is_last {
        inner = inner.border_b_1().border_color(palette::divider_color());
    }

    button()
        .test_id(&row_test_id)
        .accessibility_label(format!("Select workflow {name_for_a11y}"))
        .on_press(move || store::select_workflow_overview(&row_id))
        .hover(move |s| {
            if is_selected {
                s
            } else {
                s.bg(palette::subtle_surface_bg())
            }
        })
        .active(|s| s.opacity(0.92))
        .child(inner)
}

fn run_outcome_square(outcome: RunOutcomeDot) -> impl IntoElement {
    let base = view()
        .w(px(14.0))
        .h(px(14.0))
        .rounded(px(3.5))
        .flex_shrink_0()
        .items_center()
        .justify_center();

    match outcome {
        RunOutcomeDot::Succeeded => base
            .bg(palette::run_dot_success_bg())
            .child(icons::run_dot_check(palette::nav_active_fg())),
        RunOutcomeDot::FailedOrStopped => base
            .bg(palette::card_bg())
            .border_1()
            .border_color(palette::run_dot_failed_border())
            .child(icons::run_dot_cross(palette::run_dot_failed_border())),
        RunOutcomeDot::RunningOrWaiting => base
            .bg(palette::run_dot_running_bg())
            .child(icons::run_dot_ellipsis(palette::nav_active_fg())),
        RunOutcomeDot::NoneYet => base
            .bg(palette::card_bg())
            .border_1()
            .border_color(palette::run_dot_empty_border()),
    }
}

// --- Legend & Templates -----------------------------------------------------------------

fn run_outcome_legend() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(16.0))
        .px(px(2.0))
        .child(legend_item(RunOutcomeDot::Succeeded, "Succeeded"))
        .child(legend_item(
            RunOutcomeDot::FailedOrStopped,
            "Failed or stopped",
        ))
        .child(legend_item(
            RunOutcomeDot::RunningOrWaiting,
            "Running or waiting",
        ))
        .child(legend_item(RunOutcomeDot::NoneYet, "No run yet"))
}

fn legend_item(dot: RunOutcomeDot, label: &'static str) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(run_outcome_square(dot))
        .child(
            text(label)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}

fn templates_section() -> impl IntoElement {
    view()
        .mt(px(4.0))
        .gap(px(8.0))
        .child(
            text("Start from a template")
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_secondary()),
        )
        .child(
            view()
                .flex_row()
                .gap(px(10.0))
                .child(template_card(WorkflowTemplateKind::Blank))
                .child(template_card(WorkflowTemplateKind::FixAnIssue))
                .child(template_card(WorkflowTemplateKind::ReviewPullRequest))
                .child(import_template_card()),
        )
}

fn template_card(template: WorkflowTemplateKind) -> impl IntoElement {
    button()
        .test_id(template.test_id())
        .accessibility_label(template.title())
        .on_press(move || store::create_workflow_from_template(template))
        .flex_1()
        .min_w(px(0.0))
        .p(px(12.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .gap(px(3.0))
                .child(
                    text(template.title())
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(template.subtitle())
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
}

fn import_template_card() -> impl IntoElement {
    button()
        .test_id("wfo-template-import")
        .accessibility_label("Import from file...")
        .on_press(|| store::set_workflows_import_drawer_open(true))
        .flex_1()
        .min_w(px(0.0))
        .p(px(12.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .gap(px(3.0))
                .child(
                    text("Import from file...")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("A .bruk-workflow file")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
}

// --- Right-Hand Workflow Inspector ------------------------------------------------------

fn inspector_panel(
    edit_name: Signal<String>,
    edit_desc: Signal<String>,
    edit_is_draft: Signal<bool>,
    ad_hoc_summary: Signal<String>,
) -> impl IntoElement {
    view()
        .w(px(INSPECTOR_WIDTH))
        .flex_shrink_0()
        .bg(palette::card_bg())
        .border_l_1()
        .border_color(palette::border_color())
        .child(scroll().flex_1().child(dynamic(move || {
            let overview = store::active_workflows_overview();
            let selected = resolve_selected_workflow(&overview);
            let editing = store::workflows_editing_selected();
            let ad_hoc_open = store::workflows_ad_hoc_open();
            let more_open = store::workflows_more_menu_open();

            let Some(workflow) = selected else {
                return view().p(px(18.0)).child(
                    text("Select a workflow to inspect its steps and recent runs.")
                        .text_size(px(12.5))
                        .text_color(palette::text_muted()),
                );
            };

            let mut panel = view().p(px(16.0)).gap(px(14.0)).child(inspector_header(
                &workflow,
                editing,
                ad_hoc_open,
                more_open,
                edit_name,
                edit_desc,
                edit_is_draft,
            ));

            if editing {
                panel = panel.child(inspector_edit_card(
                    &workflow.id,
                    edit_name,
                    edit_desc,
                    edit_is_draft,
                ));
            }

            if ad_hoc_open {
                panel = panel.child(inspector_ad_hoc_card(&workflow, ad_hoc_summary));
            }

            if more_open {
                panel = panel.child(inspector_more_menu(&workflow));
            }

            panel = panel
                .child(inspector_steps_section(&workflow))
                .child(inspector_metadata_section(&workflow))
                .child(inspector_recent_runs_section(&workflow));

            panel
        })))
}

fn resolve_selected_workflow(overview: &ProjectWorkflowsOverview) -> Option<WorkflowOverviewEntry> {
    store::selected_workflow_overview_id()
        .and_then(|id| overview.workflow(&id).cloned())
        .or_else(|| overview.workflows.first().cloned())
}

fn inspector_header(
    workflow: &WorkflowOverviewEntry,
    _editing: bool,
    ad_hoc_open: bool,
    more_open: bool,
    _edit_name: Signal<String>,
    _edit_desc: Signal<String>,
    _edit_is_draft: Signal<bool>,
) -> impl IntoElement {
    let wf_id_for_editor = workflow.id.clone();

    let mut header_title = view().flex_row().items_center().gap(px(7.0)).child(
        text(workflow.name.clone())
            .text_size(px(15.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_primary()),
    );
    if let Some(badge) = workflow.origin_badge() {
        header_title = header_title.child(
            view()
                .px(px(6.5))
                .py(px(1.5))
                .rounded(px(4.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .child(
                    text(badge)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    view()
        .gap(px(4.0))
        .child(header_title)
        .child(
            text(workflow.description.clone())
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
        .child(
            view()
                .mt(px(8.0))
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .child(
                    button()
                        .test_id("wfo-inspector-edit")
                        .accessibility_label("Edit")
                        .on_press(move || {
                            store::open_workflow_editor_from_overview(&wf_id_for_editor);
                        })
                        .h(px(29.0))
                        .px(px(12.0))
                        .rounded(px(7.0))
                        .bg(palette::nav_active_bg())
                        .flex_row()
                        .items_center()
                        .gap(px(5.5))
                        .active(|s| s.opacity(0.85))
                        .child(icons::pencil(palette::nav_active_fg()))
                        .child(
                            text("Edit")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                )
                .child(
                    button()
                        .test_id("wfo-inspector-adhoc")
                        .accessibility_label("Ad-hoc Run...")
                        .on_press(move || store::set_workflows_ad_hoc_open(!ad_hoc_open))
                        .h(px(29.0))
                        .px(px(11.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Ad-hoc Run...")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    button()
                        .test_id("wfo-inspector-more")
                        .accessibility_label("More workflow actions")
                        .on_press(move || store::set_workflows_more_menu_open(!more_open))
                        .w(px(29.0))
                        .h(px(29.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(icons::more_horizontal(palette::text_secondary())),
                ),
        )
}

fn inspector_edit_card(
    workflow_id: &str,
    edit_name: Signal<String>,
    edit_desc: Signal<String>,
    edit_is_draft: Signal<bool>,
) -> impl IntoElement {
    let wf_id = workflow_id.to_owned();
    let save = move || {
        store::update_selected_overview_workflow(
            &wf_id,
            &edit_name.get_untracked(),
            &edit_desc.get_untracked(),
            edit_is_draft.get_untracked(),
        );
    };

    view()
        .p(px(11.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette::card_selected_border())
        .bg(palette::card_inset_bg())
        .gap(px(8.0))
        .child(
            text("Edit workflow")
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(labeled_input(
            "wfo-edit-name",
            "Name",
            "Workflow name",
            edit_name,
        ))
        .child(labeled_input(
            "wfo-edit-desc",
            "Description",
            "Short summary",
            edit_desc,
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .child(
                    button()
                        .test_id("wfo-edit-draft-toggle")
                        .on_press(move || edit_is_draft.set(!edit_is_draft.get_untracked()))
                        .px(px(8.0))
                        .py(px(4.5))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .child(
                            text(move || {
                                if edit_is_draft.get() {
                                    "Draft"
                                } else {
                                    "Published"
                                }
                            })
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_secondary()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            button()
                                .on_press(|| store::set_workflows_editing_selected(false))
                                .px(px(9.0))
                                .py(px(4.5))
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .child(
                                    text("Cancel")
                                        .text_size(px(11.5))
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("wfo-edit-save")
                                .accessibility_label("Save Changes")
                                .on_press(save)
                                .px(px(10.0))
                                .py(px(4.5))
                                .rounded(px(6.0))
                                .bg(palette::nav_active_bg())
                                .child(
                                    text("Save")
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::nav_active_fg()),
                                ),
                        ),
                ),
        )
}

fn inspector_ad_hoc_card(
    workflow: &WorkflowOverviewEntry,
    ad_hoc_summary: Signal<String>,
) -> impl IntoElement {
    let wf_id = workflow.id.clone();
    let start = move || {
        let prompt = ad_hoc_summary.get_untracked();
        if !prompt.trim().is_empty() {
            ad_hoc_summary.set(String::new());
        }
        let _ = store::launch_overview_ad_hoc_run(&wf_id, &prompt);
    };

    view()
        .p(px(11.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette::card_selected_border())
        .bg(palette::card_inset_bg())
        .gap(px(8.0))
        .child(
            text(format!("Run {}", workflow.name))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(labeled_input(
            "wfo-adhoc-input",
            "Task or issue reference",
            "e.g. Issue #342 · Verify retry backoff",
            ad_hoc_summary,
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(6.0))
                .child(
                    button()
                        .on_press(|| store::set_workflows_ad_hoc_open(false))
                        .px(px(9.0))
                        .py(px(4.5))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .child(
                            text("Cancel")
                                .text_size(px(11.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    button()
                        .test_id("wfo-adhoc-start")
                        .accessibility_label("Start Run")
                        .on_press(start)
                        .px(px(10.0))
                        .py(px(4.5))
                        .rounded(px(6.0))
                        .bg(palette::nav_active_bg())
                        .child(
                            text("Start Run")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        )
}

fn inspector_more_menu(workflow: &WorkflowOverviewEntry) -> impl IntoElement {
    let wf_id_dup = workflow.id.clone();
    let wf_id_draft = workflow.id.clone();
    let wf_id_export = workflow.id.clone();
    let wf_id_delete = workflow.id.clone();
    let draft_action_label = if workflow.is_draft {
        "Publish workflow"
    } else {
        "Move to draft"
    };

    view()
        .p(px(6.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .gap(px(2.0))
        .child(menu_action_row(
            "wfo-action-duplicate",
            "Duplicate workflow",
            false,
            move || store::duplicate_overview_workflow(&wf_id_dup),
        ))
        .child(menu_action_row(
            "wfo-action-toggle-draft",
            draft_action_label,
            false,
            move || store::toggle_overview_workflow_draft(&wf_id_draft),
        ))
        .child(menu_action_row(
            "wfo-action-export",
            "Export .bruk-workflow",
            false,
            move || store::export_overview_workflow(&wf_id_export),
        ))
        .child(menu_action_row(
            "wfo-action-delete",
            "Delete workflow",
            true,
            move || store::delete_overview_workflow(&wf_id_delete),
        ))
}

fn menu_action_row(
    test_id: &'static str,
    label: &'static str,
    destructive: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .on_press(on_press)
        .px(px(9.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if destructive {
                    palette::destructive_text()
                } else {
                    palette::text_primary()
                }),
        )
}

fn inspector_steps_section(workflow: &WorkflowOverviewEntry) -> impl IntoElement {
    let mut list = view().gap(px(5.0)).child(
        text("Steps")
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::text_muted()),
    );

    for group in &workflow.step_groups {
        match group {
            WorkflowStepGroup::Single(step) => {
                list = list.child(step_row(step, false));
            }
            WorkflowStepGroup::Loop {
                title,
                condition_label,
                steps,
            } => {
                let mut loop_block = view().gap(px(4.0)).child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.5))
                        .bg(palette::card_selected_bg())
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child(icons::loop_arrows(palette::link_blue()))
                                .child(
                                    text(title.clone())
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            text(condition_label.clone())
                                .flex_shrink_0()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::link_blue()),
                        ),
                );
                for child_step in steps {
                    loop_block = loop_block.child(step_row(child_step, true));
                }
                list = list.child(loop_block);
            }
        }
    }

    list
}

fn step_row(step: &WorkflowStepPreview, indented: bool) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .pl(if indented { px(16.0) } else { px(4.0) })
        .pr(px(4.0))
        .py(px(3.5))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    text(step.number.to_string())
                        .w(px(12.0))
                        .text_size(px(11.5))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(step.name.clone())
                        .flex_1()
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            text(step.actor.clone())
                .flex_shrink_0()
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}

fn inspector_metadata_section(workflow: &WorkflowOverviewEntry) -> impl IntoElement {
    view()
        .pt(px(12.0))
        .border_t_1()
        .border_color(palette::divider_color())
        .gap(px(7.5))
        .child(meta_kv_row("Picks up", workflow.picks_up_label.clone()))
        .child(meta_kv_row("Ad-hoc runs", workflow.ad_hoc_label.clone()))
        .child(meta_kv_row("Median run", workflow.median_run_label.clone()))
        .child(meta_kv_row("Average cost", workflow.average_cost_display()))
        .child(meta_kv_row("Edited", workflow.edited_label.clone()))
}

fn meta_kv_row(label: &'static str, value: String) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            text(label)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(
            text(value)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .text_color(palette::text_primary()),
        )
}

fn inspector_recent_runs_section(workflow: &WorkflowOverviewEntry) -> impl IntoElement {
    let mut section = view()
        .pt(px(12.0))
        .border_t_1()
        .border_color(palette::divider_color())
        .gap(px(6.5))
        .child(
            text("Recent runs")
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        );

    if workflow.recent_runs.is_empty() {
        section = section.child(
            view()
                .px(px(10.0))
                .py(px(10.0))
                .rounded(px(7.5))
                .border_1()
                .border_color(palette::divider_color())
                .bg(palette::card_inset_bg())
                .child(
                    text("No runs yet")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        );
    } else {
        for run in &workflow.recent_runs {
            section = section.child(recent_run_row(&workflow.name, run));
        }
    }

    section
}

fn recent_run_row(workflow_name: &str, run: &WorkflowRecentRun) -> impl IntoElement {
    let wf_name = workflow_name.to_owned();
    let run_test_id = format!("wfo-recent-run-{}", run.run_number);
    let status_color = match run.status {
        RecentRunStatus::Running => palette::link_blue(),
        RecentRunStatus::NeedsYou => palette::workflow_now_amber(),
        RecentRunStatus::Done => palette::success_text(),
        RecentRunStatus::Failed => palette::destructive_text(),
    };

    button()
        .test_id(&run_test_id)
        .accessibility_label(format!("Open run #{} {}", run.run_number, run.summary))
        .on_press(move || store::open_workflow_run_from_overview(&wf_name))
        .rounded(px(7.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(8.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text(format!("#{}", run.run_number))
                                .flex_shrink_0()
                                .text_size(px(11.5))
                                .tabular_nums()
                                .text_color(palette::text_muted()),
                        )
                        .child(
                            text(run.summary.clone())
                                .flex_1()
                                .text_size(px(11.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    text(run.status.label())
                        .flex_shrink_0()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(status_color),
                ),
        )
}
