//! Visual Workflow Editor page: breadcrumb header with unsaved badge & actions,
//! left step-types palette, centered visual flow canvas with loop & fallback branch,
//! and contextual right-hand inspector matching `13 WorkflowEditor.png`.

use factory_core::{
    EditorLoopBlock, EditorNode, EditorSelection, EditorStep, ExitField, ExitOperator,
    LoopExhaustedAction, LoopExitCondition, LoopRepeatMode, LoopWaitBetween, PaletteCategory,
    StepTypeKind, WorkflowEditorDocument,
};
use nsh_base::{SelectItem, SelectValue, button, input, select};
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{icons, palette},
};

const PALETTE_WIDTH: f32 = 252.0;
const INSPECTOR_WIDTH: f32 = 316.0;
const CANVAS_FLOW_WIDTH: f32 = 564.0;
const FALLBACK_BRANCH_WIDTH: f32 = 186.0;

// --- Top Header Bar (Full-Width Editor Mode) --------------------------------------------

/// Top header bar shown when `ActiveView::WorkflowEditor` is active.
pub(crate) fn workflow_editor_header() -> impl IntoElement {
    view()
        .h(px(54.0))
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .justify_between()
        .pl(move || {
            let inset = nsh_core::element::desktop::title_bar().left.0;
            if inset > 0.0 {
                px(inset + 10.0)
            } else {
                px(14.0)
            }
        })
        .pr(px(18.0))
        .bg(palette::header_bg())
        .border_b_1()
        .border_color(palette::border_color())
        .window_drag(true)
        .child(header_breadcrumb_cluster())
        .child(header_actions_cluster())
}

fn header_breadcrumb_cluster() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .window_drag(false)
        .child(
            button()
                .test_id("wfe-header-back")
                .accessibility_label("Back to Workflows")
                .on_press(store::close_workflow_editor)
                .w(px(26.0))
                .h(px(26.0))
                .rounded(px(6.0))
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.75))
                .child(icons::chevron_left(palette::text_secondary())),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    button()
                        .test_id("wfe-breadcrumb-project")
                        .accessibility_label("Project Workflows")
                        .on_press(store::open_workflows_from_editor)
                        .rounded(px(4.5))
                        .px(px(3.0))
                        .py(px(2.0))
                        .hover(|s| s.bg(palette::sidebar_hover_bg()))
                        .child(
                            text(move || store::active_workflow_editor().project_name)
                                .text_size(px(13.0))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    text("›")
                        .text_size(px(13.0))
                        .text_color(palette::text_muted()),
                )
                .child(
                    button()
                        .test_id("wfe-breadcrumb-workflows")
                        .accessibility_label("Workflows")
                        .on_press(store::open_workflows_from_editor)
                        .rounded(px(4.5))
                        .px(px(3.0))
                        .py(px(2.0))
                        .hover(|s| s.bg(palette::sidebar_hover_bg()))
                        .child(
                            text("Workflows")
                                .text_size(px(13.0))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    text("›")
                        .text_size(px(13.0))
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(move || store::active_workflow_editor().workflow_name)
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(when(
            || store::active_workflow_editor().has_unsaved_changes,
            || {
                view()
                    .test_id("wfe-unsaved-badge")
                    .ml(px(4.0))
                    .px(px(8.0))
                    .py(px(2.5))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(palette::editor_unsaved_border())
                    .bg(palette::editor_unsaved_bg())
                    .child(
                        text("Unsaved changes")
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::editor_unsaved_text()),
                    )
            },
        ))
}

fn header_actions_cluster() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .window_drag(false)
        .child(
            button()
                .test_id("wfe-header-validate")
                .accessibility_label("Validate")
                .on_press(store::validate_workflow_editor)
                .h(px(30.0))
                .px(px(11.0))
                .rounded(px(7.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(icons::check(palette::text_secondary()))
                .child(
                    text("Validate")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .test_id("wfe-header-history")
                .accessibility_label("Version History")
                .on_press(store::toggle_workflow_editor_history)
                .w(px(30.0))
                .h(px(30.0))
                .rounded(px(7.5))
                .border_1()
                .border_color(move || {
                    if store::workflow_editor_history_open() {
                        palette::card_selected_border()
                    } else {
                        palette::border_color()
                    }
                })
                .bg(move || {
                    if store::workflow_editor_history_open() {
                        palette::card_selected_bg()
                    } else {
                        palette::control_bg()
                    }
                })
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(icons::clock_progress(14.0, palette::text_secondary())),
        )
        .child(
            button()
                .test_id("wfe-header-save")
                .accessibility_label("Save")
                .on_press(store::save_workflow_editor)
                .h(px(30.0))
                .px(px(12.0))
                .rounded(px(7.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Save")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .test_id("wfe-header-adhoc")
                .accessibility_label("Ad-hoc Run...")
                .on_press(|| {
                    let open = store::workflow_editor_ad_hoc_open();
                    store::set_workflow_editor_ad_hoc_open(!open);
                })
                .h(px(30.0))
                .px(px(12.0))
                .rounded(px(7.5))
                .bg(palette::nav_active_bg())
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .active(|s| s.opacity(0.85))
                .child(icons::play_filled(palette::nav_active_fg()))
                .child(
                    text("Ad-hoc Run...")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        )
}

// --- Main Three-Pane Layout -------------------------------------------------------------

/// Main visual workflow editor surface: step palette, visual flow canvas, and right inspector.
pub(crate) fn workflow_editor_view() -> impl IntoElement {
    let ad_hoc_prompt = signal(String::new());

    view()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .flex_row()
        .bg(palette::canvas_bg())
        .child(left_step_palette())
        .child(center_workflow_canvas(ad_hoc_prompt))
        .child(when(
            store::workflow_editor_inspector_visible,
            right_inspector_pane,
        ))
}

// --- Left Pane: Step Types Palette ------------------------------------------------------

fn left_step_palette() -> impl IntoElement {
    view()
        .w(px(PALETTE_WIDTH))
        .flex_shrink_0()
        .bg(palette::sidebar_bg())
        .border_r_1()
        .border_color(palette::border_color())
        .child(
            view().px(px(12.0)).pt(px(12.0)).pb(px(10.0)).child(
                view()
                    .h(px(30.0))
                    .px(px(9.0))
                    .rounded(px(7.5))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::control_bg())
                    .flex_row()
                    .items_center()
                    .gap(px(7.0))
                    .child(icons::search(palette::text_muted()))
                    .child(
                        input()
                            .test_id("wfe-palette-search")
                            .value(store::workflow_editor_palette_search)
                            .on_change(|next: &str| {
                                store::set_workflow_editor_palette_search(next);
                            })
                            .placeholder("Filter step types")
                            .flex_1()
                            .min_w(px(0.0))
                            .bg(AdaptiveColor::TRANSPARENT)
                            .text_size(px(12.0))
                            .text_color(palette::text_primary()),
                    ),
            ),
        )
        .child(scroll().flex_1().child(dynamic(|| {
            let query = store::workflow_editor_palette_search();
            let highlighted = store::active_workflow_editor().highlighted_palette_kind();

            let mut list = view().px(px(12.0)).pb(px(18.0)).gap(px(14.0));
            let mut matched_any = false;

            for category in PaletteCategory::ALL {
                let items: Vec<StepTypeKind> = StepTypeKind::ALL
                    .iter()
                    .copied()
                    .filter(|kind| kind.category() == *category && kind.matches_query(&query))
                    .collect();

                if items.is_empty() {
                    continue;
                }
                matched_any = true;

                let mut section = view().gap(px(6.0)).child(
                    text(category.label())
                        .px(px(2.0))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                );

                for kind in items {
                    let is_highlighted = highlighted == Some(kind);
                    section = section.child(palette_step_card(kind, is_highlighted));
                }
                list = list.child(section);
            }

            if !matched_any {
                list = list.child(
                    view().py(px(20.0)).px(px(8.0)).items_center().child(
                        text("No matching step types")
                            .text_size(px(12.0))
                            .text_color(palette::text_muted()),
                    ),
                );
            }

            list
        })))
}

fn palette_step_card(kind: StepTypeKind, is_highlighted: bool) -> impl IntoElement {
    let badge: AnyElement = if kind == StepTypeKind::Loop {
        view()
            .w(px(26.0))
            .h(px(26.0))
            .rounded(px(6.5))
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .bg(if is_highlighted {
                palette::editor_loop_pill_bg()
            } else {
                palette::editor_palette_badge_bg()
            })
            .child(icons::loop_arrows(if is_highlighted {
                palette::link_blue()
            } else {
                palette::text_secondary()
            }))
            .into_element()
    } else if kind.is_composite() {
        view()
            .w(px(26.0))
            .h(px(26.0))
            .rounded(px(6.5))
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .bg(palette::editor_palette_badge_bg())
            .child(icons::composite_grid(palette::text_muted()))
            .into_element()
    } else {
        let monogram = kind.badge_monogram().unwrap_or("St");
        view()
            .w(px(26.0))
            .h(px(26.0))
            .rounded(px(6.5))
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .bg(if is_highlighted {
                palette::editor_loop_pill_bg()
            } else {
                palette::editor_palette_badge_bg()
            })
            .child(
                text(monogram)
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if is_highlighted {
                        palette::link_blue()
                    } else {
                        palette::text_secondary()
                    }),
            )
            .into_element()
    };

    button()
        .test_id(kind.test_id())
        .accessibility_label(format!("Add {} step", kind.title()))
        .on_press(move || store::add_workflow_editor_step(kind))
        .rounded(px(8.5))
        .border_1()
        .border_color(if is_highlighted {
            palette::card_selected_border()
        } else {
            palette::border_color()
        })
        .bg(if is_highlighted {
            palette::editor_loop_bg()
        } else {
            palette::card_bg()
        })
        .hover(move |s| {
            if is_highlighted {
                s
            } else {
                s.bg(palette::subtle_surface_bg())
            }
        })
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(9.0))
                .px(px(9.0))
                .py(px(8.0))
                .child(badge)
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(1.5))
                        .child(
                            text(kind.title())
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(kind.subtitle())
                                .text_size(px(10.5))
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
}

// --- Center Pane: Visual Workflow Canvas ------------------------------------------------

fn center_workflow_canvas(ad_hoc_prompt: Signal<String>) -> impl IntoElement {
    scroll()
        .flex_1()
        .min_w(px(0.0))
        .bg(palette::canvas_bg())
        .child(dynamic(move || {
            let doc = store::active_workflow_editor();
            let validation = store::workflow_editor_validation();
            let history_open = store::workflow_editor_history_open();
            let ad_hoc_open = store::workflow_editor_ad_hoc_open();

            let mut inner = view().w(px(CANVAS_FLOW_WIDTH)).gap(px(0.0));

            if let Some(report) = validation {
                inner = inner.child(view().mb(px(14.0)).child(validation_banner(
                    report.is_valid,
                    &report.summary,
                    &report.issues,
                )));
            }

            if history_open {
                inner = inner.child(view().mb(px(14.0)).child(version_history_card(&doc)));
            }

            if ad_hoc_open {
                inner = inner.child(
                    view()
                        .mb(px(14.0))
                        .child(editor_ad_hoc_card(&doc.workflow_name, ad_hoc_prompt)),
                );
            }

            inner = inner.child(trigger_header_card(&doc));
            inner = inner.child(canvas_nodes_spine(&doc));

            view()
                .flex_1()
                .items_center()
                .px(px(28.0))
                .py(px(22.0))
                .child(inner)
        }))
}

fn validation_banner(is_valid: bool, summary: &str, issues: &[String]) -> impl IntoElement {
    let mut card = view()
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(if is_valid {
            palette::success_text()
        } else {
            palette::destructive_text()
        })
        .bg(if is_valid {
            palette::status_success_bg()
        } else {
            palette::status_error_bg()
        })
        .gap(px(4.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    view()
                        .flex_1()
                        .flex_row()
                        .items_center()
                        .gap(px(7.0))
                        .child(if is_valid {
                            icons::check(palette::success_text()).into_element()
                        } else {
                            icons::alert_circle(palette::destructive_text()).into_element()
                        })
                        .child(
                            text(summary.to_owned())
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if is_valid {
                                    palette::success_text()
                                } else {
                                    palette::destructive_text()
                                }),
                        ),
                )
                .child(
                    button()
                        .test_id("wfe-validation-dismiss")
                        .accessibility_label("Dismiss validation result")
                        .on_press(store::dismiss_workflow_editor_validation)
                        .w(px(20.0))
                        .h(px(20.0))
                        .rounded(px(4.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::close_x(palette::text_muted())),
                ),
        );

    for issue in issues {
        card = card.child(
            text(format!("• {issue}"))
                .pl(px(20.0))
                .text_size(px(11.5))
                .text_color(palette::text_secondary()),
        );
    }

    card
}

fn version_history_card(doc: &WorkflowEditorDocument) -> impl IntoElement {
    let mut card = view()
        .p(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("Version history")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .accessibility_label("Close version history")
                        .on_press(store::toggle_workflow_editor_history)
                        .w(px(20.0))
                        .h(px(20.0))
                        .rounded(px(4.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::close_x(palette::text_muted())),
                ),
        );

    for rev in &doc.revisions {
        card = card.child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .px(px(8.0))
                .py(px(6.0))
                .rounded(px(6.5))
                .bg(palette::card_inset_bg())
                .child(
                    view()
                        .flex_1()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(format!("v{}", rev.version))
                                .app_class("font-mono")
                                .text_size(px(11.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::link_blue()),
                        )
                        .child(
                            text(rev.label.clone())
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    text(rev.timestamp.clone())
                        .text_size(px(11.0))
                        .text_color(palette::text_muted()),
                ),
        );
    }

    card
}

fn editor_ad_hoc_card(workflow_name: &str, ad_hoc_prompt: Signal<String>) -> impl IntoElement {
    let start = move || {
        let prompt = ad_hoc_prompt.get_untracked();
        if store::launch_editor_ad_hoc_run(&prompt) {
            ad_hoc_prompt.set(String::new());
        }
    };

    view()
        .p(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::card_selected_border())
        .bg(palette::card_bg())
        .gap(px(8.0))
        .child(
            text(format!("Ad-hoc run · {workflow_name}"))
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            input()
                .test_id("wfe-adhoc-input")
                .value(move || ad_hoc_prompt.get())
                .on_change(move |next: &str| ad_hoc_prompt.set(next.to_owned()))
                .placeholder("e.g. Issue #342 · Verify retry backoff")
                .h(px(30.0))
                .px(px(9.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .text_size(px(12.0))
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .child(
                    button()
                        .on_press(|| store::set_workflow_editor_ad_hoc_open(false))
                        .px(px(10.0))
                        .py(px(5.0))
                        .rounded(px(6.5))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .child(
                            text("Cancel")
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    button()
                        .test_id("wfe-adhoc-start")
                        .accessibility_label("Start Run")
                        .on_press(start)
                        .px(px(11.0))
                        .py(px(5.0))
                        .rounded(px(6.5))
                        .bg(palette::nav_active_bg())
                        .child(
                            text("Start Run")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        )
}

fn trigger_header_card(doc: &WorkflowEditorDocument) -> impl IntoElement {
    let selected = doc.selected == EditorSelection::Trigger;

    let mut pills = view().flex_row().items_center().gap(px(8.0));
    if doc.trigger.work_queue_enabled {
        pills = pills.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .px(px(10.0))
                .py(px(4.5))
                .rounded_full()
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::subtle_surface_bg())
                .child(icons::work_queue(palette::text_secondary()))
                .child(
                    text(doc.trigger.work_queue_label.clone())
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        );
    }
    if doc.trigger.ad_hoc_allowed {
        pills = pills.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .px(px(10.0))
                .py(px(4.5))
                .rounded_full()
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::subtle_surface_bg())
                .child(icons::play_filled(palette::text_secondary()))
                .child(
                    text("Ad-hoc runs allowed")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        );
    }
    if !doc.trigger.work_queue_enabled && !doc.trigger.ad_hoc_allowed {
        pills = pills.child(
            text("No triggers enabled")
                .text_size(px(12.0))
                .text_color(palette::destructive_text()),
        );
    }

    let card = button()
        .test_id("wfe-node-trigger")
        .accessibility_label("Workflow triggers")
        .on_press(|| store::select_workflow_editor_node(EditorSelection::Trigger))
        .w_full()
        .rounded(px(12.0))
        .bg(palette::card_inset_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.9));

    let card = if selected {
        card.border_2()
            .border_color(palette::card_selected_border())
    } else {
        card.border_1().border_color(palette::border_color())
    };

    card.child(
        view()
            .w_full()
            .px(px(14.0))
            .py(px(12.0))
            .gap(px(8.0))
            .child(
                text("Starts from")
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_muted()),
            )
            .child(pills)
            .child(
                text(doc.trigger.inputs_summary.clone())
                    .app_class("font-mono")
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
            ),
    )
}

fn canvas_nodes_spine(doc: &WorkflowEditorDocument) -> impl IntoElement {
    let mut spine = view().w_full().gap(px(0.0));
    let mut idx = 0;

    while idx < doc.nodes.len() {
        match &doc.nodes[idx] {
            EditorNode::Step(step) => {
                let number = doc.step_number(&step.id).unwrap_or(1);
                let is_selected = doc.selected == EditorSelection::Step(step.id.clone());
                spine = spine.child(vertical_connector(18.0)).child(step_card(
                    step,
                    number,
                    is_selected,
                ));
                idx += 1;
            }
            EditorNode::Loop(block) => {
                let is_loop_selected = doc.selected == EditorSelection::Loop(block.id.clone());
                spine = spine
                    .child(vertical_connector(18.0))
                    .child(loop_container_card(doc, block, is_loop_selected));

                let has_fallback_branch =
                    block.on_exhausted == LoopExhaustedAction::GoToFallbackStep;

                // Collect any immediately trailing Step nodes so they align cleanly beside the
                // right-hand `If out of attempts` fallback branch, matching `13 WorkflowEditor.png`.
                let mut trailing_steps: Vec<&EditorStep> = Vec::new();
                let mut lookahead = idx + 1;
                while lookahead < doc.nodes.len() {
                    if let EditorNode::Step(next_step) = &doc.nodes[lookahead] {
                        trailing_steps.push(next_step);
                        lookahead += 1;
                    } else {
                        break;
                    }
                }

                if has_fallback_branch && !trailing_steps.is_empty() {
                    let is_last_segment = lookahead == doc.nodes.len();
                    spine = spine.child(post_loop_split_section(
                        doc,
                        block,
                        &trailing_steps,
                        is_last_segment,
                    ));
                    idx = lookahead;
                    if is_last_segment {
                        return spine;
                    }
                } else {
                    if has_fallback_branch {
                        let is_fb_selected =
                            doc.selected == EditorSelection::Fallback(block.id.clone());
                        spine = spine.child(
                            view().w_full().flex_row().justify_end().child(
                                view()
                                    .w(px(FALLBACK_BRANCH_WIDTH))
                                    .child(amber_connector(14.0))
                                    .child(fallback_branch_card(block, is_fb_selected)),
                            ),
                        );
                    }
                    idx += 1;
                }
            }
        }
    }

    spine.child(
        view()
            .w_full()
            .items_center()
            .pt(px(16.0))
            .child(add_step_canvas_button()),
    )
}

fn post_loop_split_section(
    doc: &WorkflowEditorDocument,
    block: &EditorLoopBlock,
    trailing_steps: &[&EditorStep],
    include_add_button: bool,
) -> impl IntoElement {
    let mut left_col = view().flex_1().min_w(px(0.0)).gap(px(0.0));
    for step in trailing_steps {
        let number = doc.step_number(&step.id).unwrap_or(1);
        let is_selected = doc.selected == EditorSelection::Step(step.id.clone());
        left_col =
            left_col
                .child(vertical_connector(16.0))
                .child(step_card(step, number, is_selected));
    }
    if include_add_button {
        left_col = left_col.child(
            view()
                .w_full()
                .items_center()
                .pt(px(16.0))
                .child(add_step_canvas_button()),
        );
    }

    let is_fb_selected = doc.selected == EditorSelection::Fallback(block.id.clone());
    let right_col = view()
        .w(px(FALLBACK_BRANCH_WIDTH))
        .flex_shrink_0()
        .child(amber_connector(16.0))
        .child(fallback_branch_card(block, is_fb_selected));

    view()
        .w_full()
        .flex_row()
        .items_start()
        .gap(px(14.0))
        .child(left_col)
        .child(right_col)
}

fn vertical_connector(height: f32) -> impl IntoElement {
    view().w_full().items_center().child(
        view()
            .w(px(1.5))
            .h(px(height))
            .bg(palette::editor_connector_color()),
    )
}

fn amber_connector(height: f32) -> impl IntoElement {
    view().w_full().items_center().child(
        view()
            .w(px(1.5))
            .h(px(height))
            .bg(palette::editor_fallback_border()),
    )
}

fn step_card(step: &EditorStep, number: u32, is_selected: bool) -> impl IntoElement {
    let step_id = step.id.clone();
    let test_id = format!("wfe-node-{}", step.id);

    let mut content = view()
        .w_full()
        .px(px(14.0))
        .py(px(11.0))
        .gap(px(5.0))
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            view()
                                .w(px(22.0))
                                .h(px(22.0))
                                .rounded_full()
                                .flex_shrink_0()
                                .items_center()
                                .justify_center()
                                .bg(palette::subtle_surface_bg())
                                .child(
                                    text(number.to_string())
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .tabular_nums()
                                        .text_color(palette::text_muted()),
                                ),
                        )
                        .child(
                            text(step.name.clone())
                                .flex_1()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .flex_shrink_0()
                        .px(px(8.5))
                        .py(px(3.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::divider_color())
                        .bg(palette::subtle_surface_bg())
                        .child(
                            text(step.actor_badge.clone())
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_muted()),
                        ),
                ),
        );

    if let Some(subtitle) = &step.subtitle {
        content = content.child(
            text(subtitle.clone())
                .pl(px(32.0))
                .app_class("font-mono")
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        );
    }

    let card = button()
        .test_id(&test_id)
        .accessibility_label(format!("Step {number}: {}", step.name))
        .on_press(move || {
            store::select_workflow_editor_node(EditorSelection::Step(step_id.clone()))
        })
        .w_full()
        .rounded(px(10.0))
        .bg(palette::card_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.9));

    let card = if is_selected {
        card.border_2()
            .border_color(palette::card_selected_border())
    } else {
        card.border_1().border_color(palette::border_color())
    };

    card.child(content)
}

fn loop_container_card(
    doc: &WorkflowEditorDocument,
    block: &EditorLoopBlock,
    is_selected: bool,
) -> impl IntoElement {
    let loop_id_header = block.id.clone();
    let loop_id_footer = block.id.clone();
    let test_id = format!("wfe-node-{}", block.id);
    let (exhausted_prefix, exhausted_target) = block.exhausted_prefix_and_target();

    let mut inner_steps_col = view().flex_1().min_w(px(0.0)).gap(px(0.0));
    for (idx, child_step) in block.steps.iter().enumerate() {
        if idx > 0 {
            inner_steps_col = inner_steps_col.child(vertical_connector(10.0));
        }
        let number = doc.step_number(&child_step.id).unwrap_or(1);
        let child_selected = doc.selected == EditorSelection::Step(child_step.id.clone());
        inner_steps_col = inner_steps_col.child(step_card(child_step, number, child_selected));
    }

    let outer = view()
        .w_full()
        .rounded(px(12.0))
        .bg(palette::editor_loop_bg())
        .overflow_hidden();

    let outer = if is_selected {
        outer
            .border_2()
            .border_color(palette::card_selected_border())
    } else {
        outer
            .border_1()
            .border_color(palette::editor_loop_divider())
    };

    outer
        .child(
            button()
                .test_id(&test_id)
                .accessibility_label(format!("Loop: {}", block.name))
                .on_press(move || {
                    store::select_workflow_editor_node(EditorSelection::Loop(
                        loop_id_header.clone(),
                    ));
                })
                .w_full()
                .px(px(14.0))
                .py(px(11.0))
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .hover(|s| s.opacity(0.95))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(icons::loop_arrows(palette::link_blue()))
                        .child(
                            text(block.name.clone())
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .flex_shrink_0()
                        .px(px(10.0))
                        .py(px(3.5))
                        .rounded_full()
                        .bg(palette::editor_loop_pill_bg())
                        .child(
                            text(block.header_badge_label())
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .tabular_nums()
                                .text_color(palette::link_blue()),
                        ),
                ),
        )
        .child(
            view()
                .w_full()
                .px(px(12.0))
                .pb(px(12.0))
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .child(icons::loop_return_arrow(palette::card_selected_border()))
                .child(inner_steps_col),
        )
        .child(
            button()
                .accessibility_label("Select loop exit conditions")
                .on_press(move || {
                    store::select_workflow_editor_node(EditorSelection::Loop(
                        loop_id_footer.clone(),
                    ));
                })
                .w_full()
                .border_t_1()
                .border_color(palette::editor_loop_divider())
                .px(px(14.0))
                .py(px(10.0))
                .child(
                    view()
                        .w_full()
                        .gap(px(5.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(6.5))
                                .child(icons::check(palette::success_text()))
                                .child(
                                    text("Exit when")
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::success_text()),
                                )
                                .child(
                                    text(block.exit_expression())
                                        .app_class("font-mono")
                                        .text_size(px(11.5))
                                        .text_color(palette::text_secondary()),
                                ),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(6.5))
                                .child(icons::alert_circle(palette::editor_unsaved_text()))
                                .child(
                                    text("Out of attempts")
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::editor_unsaved_text()),
                                )
                                .child(
                                    text(exhausted_prefix)
                                        .text_size(px(11.5))
                                        .text_color(palette::text_muted()),
                                )
                                .child(
                                    text(exhausted_target)
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                ),
        )
}

fn fallback_branch_card(block: &EditorLoopBlock, is_selected: bool) -> impl IntoElement {
    let loop_id = block.id.clone();

    let card = button()
        .test_id("wfe-node-fallback")
        .accessibility_label(format!("Fallback step: {}", block.fallback.name))
        .on_press(move || {
            store::select_workflow_editor_node(EditorSelection::Fallback(loop_id.clone()));
        })
        .w_full()
        .rounded(px(10.0))
        .bg(palette::editor_fallback_bg())
        .hover(|s| s.opacity(0.92))
        .active(|s| s.opacity(0.85));

    let card = if is_selected {
        card.border_2().border_color(palette::editor_unsaved_text())
    } else {
        card.border_1()
            .border_color(palette::editor_fallback_border())
    };

    card.child(
        view()
            .w_full()
            .px(px(12.0))
            .py(px(11.0))
            .gap(px(4.0))
            .child(
                text("If out of attempts")
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::editor_unsaved_text()),
            )
            .child(
                text(block.fallback.name.clone())
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_primary()),
            )
            .child(
                text(format!(
                    "{} · {}",
                    block.fallback.action_label, block.fallback.outcome_label
                ))
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
            ),
    )
}

fn add_step_canvas_button() -> impl IntoElement {
    button()
        .test_id("wfe-canvas-add-step")
        .accessibility_label("Add step")
        .on_press(|| store::add_workflow_editor_step(StepTypeKind::ClaudeTask))
        .h(px(30.0))
        .px(px(14.0))
        .rounded_full()
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(icons::plus(palette::text_muted()))
        .child(
            text("Add step")
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
}

// --- Right Pane: Contextual Inspector ---------------------------------------------------

fn right_inspector_pane() -> impl IntoElement {
    let selection = computed(|| store::active_workflow_editor().selected);

    view()
        .w(px(INSPECTOR_WIDTH))
        .flex_shrink_0()
        .bg(palette::sidebar_bg())
        .border_l_1()
        .border_color(palette::border_color())
        .child(
            scroll()
                .flex_1()
                .child(dynamic(move || match selection.get() {
                    EditorSelection::Loop(loop_id) => loop_inspector(&loop_id).into_element(),
                    EditorSelection::Step(step_id) => step_inspector(&step_id).into_element(),
                    EditorSelection::Fallback(loop_id) => {
                        fallback_inspector(&loop_id).into_element()
                    }
                    EditorSelection::Trigger => trigger_inspector().into_element(),
                })),
        )
}

// --- Loop Inspector (Matches `13 WorkflowEditor.png`) -----------------------------------

fn loop_inspector(loop_id: &str) -> impl IntoElement {
    let id_header = loop_id.to_owned();
    let id_name_val = loop_id.to_owned();
    let id_name_chg = loop_id.to_owned();
    let id_repeat = loop_id.to_owned();
    let id_conds = loop_id.to_owned();
    let id_add_cond = loop_id.to_owned();
    let id_max_val = loop_id.to_owned();
    let id_max_chg = loop_id.to_owned();
    let id_wait = loop_id.to_owned();
    let id_pass = loop_id.to_owned();
    let id_discard = loop_id.to_owned();
    let id_exhausted = loop_id.to_owned();

    view()
        .w_full()
        .child(
            // Top Inspector header bar: `Loop · wraps steps 2–3`
            view()
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .px(px(16.0))
                .py(px(12.0))
                .border_b_1()
                .border_color(palette::border_color())
                .child(icons::loop_arrows(palette::link_blue()))
                .child(
                    text(move || {
                        let doc = store::active_workflow_editor();
                        match doc.loop_step_range(&id_header) {
                            Some((first, last)) if first == last => {
                                format!("Loop · wraps step {first}")
                            }
                            Some((first, last)) => format!("Loop · wraps steps {first}–{last}"),
                            None => "Loop · empty".to_owned(),
                        }
                    })
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .p(px(16.0))
                .gap(px(16.0))
                // Name field
                .child(
                    view()
                        .gap(px(6.0))
                        .child(inspector_field_label("Name"))
                        .child(
                            input()
                                .test_id("wfe-loop-name")
                                .value(move || {
                                    store::active_workflow_editor()
                                        .find_loop(&id_name_val)
                                        .map(|l| l.name.clone())
                                        .unwrap_or_default()
                                })
                                .on_change(move |next: &str| {
                                    store::set_workflow_editor_loop_name(&id_name_chg, next);
                                })
                                .h(px(32.0))
                                .px(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                // Repeat radio group
                .child(loop_repeat_section(id_repeat))
                // Exit when condition builder
                .child(loop_exit_when_section(id_conds, id_add_cond))
                // Max attempts + Wait between (2-column)
                .child(
                    view()
                        .flex_row()
                        .items_start()
                        .gap(px(10.0))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(6.0))
                                .child(inspector_field_label("Max attempts"))
                                .child(
                                    input()
                                        .test_id("wfe-loop-max-attempts")
                                        .value(move || {
                                            store::active_workflow_editor()
                                                .find_loop(&id_max_val)
                                                .map(|l| l.max_attempts.to_string())
                                                .unwrap_or_else(|| "5".to_owned())
                                        })
                                        .on_change(move |next: &str| {
                                            store::set_workflow_editor_loop_max_attempts(
                                                &id_max_chg,
                                                next,
                                            );
                                        })
                                        .h(px(32.0))
                                        .px(px(10.0))
                                        .rounded(px(7.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::control_bg())
                                        .text_size(px(12.5))
                                        .tabular_nums()
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(6.0))
                                .child(inspector_field_label("Wait between"))
                                .child(loop_wait_select(id_wait)),
                        ),
                )
                // Between attempts checkboxes
                .child(loop_between_attempts_section(id_pass, id_discard))
                // If attempts run out dropdown
                .child(
                    view()
                        .gap(px(6.0))
                        .child(inspector_field_label("If attempts run out"))
                        .child(loop_on_exhausted_select(id_exhausted)),
                ),
        )
}

fn loop_repeat_section(loop_id: String) -> impl IntoElement {
    let mut group = view().gap(px(8.0)).child(inspector_field_label("Repeat"));

    for mode in LoopRepeatMode::ALL {
        let mode = *mode;
        let id_check = loop_id.clone();
        let id_press = loop_id.clone();
        let is_selected = move || {
            store::active_workflow_editor()
                .find_loop(&id_check)
                .is_some_and(|l| l.repeat_mode == mode)
        };

        let mut label_row = view().flex_row().items_center().gap(px(6.0)).child(
            text(mode.title())
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        );
        if let Some(hint) = mode.hint() {
            label_row = label_row.child(
                text(hint)
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
            );
        }

        group = group.child(
            button()
                .test_id(mode.test_id())
                .accessibility_label(mode.title())
                .on_press(move || store::set_workflow_editor_loop_repeat(&id_press, mode))
                .w_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .py(px(1.5))
                .active(|s| s.opacity(0.8))
                .child(dynamic(move || radio_dot(is_selected())))
                .child(label_row),
        );
    }

    group
}

fn loop_exit_when_section(loop_id: String, loop_id_add: String) -> impl IntoElement {
    let id_count = loop_id.clone();
    // Re-render condition row list only when condition count or IDs change, preserving input focus.
    let condition_ids = computed(move || {
        store::active_workflow_editor()
            .find_loop(&id_count)
            .map(|l| {
                l.exit_conditions
                    .iter()
                    .map(|c| c.id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });

    view()
        .gap(px(8.0))
        .child(inspector_field_label("Exit when"))
        .child(dynamic(move || {
            let ids = condition_ids.get();
            let total = ids.len();
            let mut rows = view().gap(px(7.0));
            for (idx, cond_id) in ids.into_iter().enumerate() {
                rows = rows.child(exit_condition_row(loop_id.clone(), cond_id, idx, total > 1));
            }
            rows
        }))
        .child(
            button()
                .test_id("wfe-exit-add-condition")
                .accessibility_label("Add exit condition")
                .on_press(move || store::add_workflow_editor_exit_condition(&loop_id_add))
                .py(px(2.0))
                .hover(|s| s.opacity(0.8))
                .active(|s| s.opacity(0.65))
                .child(
                    text("+ Add condition (all must pass)")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::link_blue()),
                ),
        )
}

fn exit_condition_row(
    loop_id: String,
    cond_id: String,
    index: usize,
    can_remove: bool,
) -> impl IntoElement {
    let step_select = exit_step_select(loop_id.clone(), cond_id.clone(), index);
    let field_select = exit_field_select(loop_id.clone(), cond_id.clone(), index);
    let op_select = exit_operator_select(loop_id.clone(), cond_id.clone(), index);

    let id_val = loop_id.clone();
    let cid_val = cond_id.clone();
    let id_chg = loop_id.clone();
    let cid_chg = cond_id.clone();
    let val_test_id = format!("wfe-exit-val-{index}");

    let mut row = view()
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .child(step_select)
        .child(field_select)
        .child(op_select)
        .child(
            input()
                .test_id(&val_test_id)
                .value(move || {
                    find_condition(&id_val, &cid_val)
                        .map(|c| c.expected)
                        .unwrap_or_default()
                })
                .on_change(move |next: &str| {
                    store::update_workflow_editor_exit_condition(
                        &id_chg,
                        &cid_chg,
                        None,
                        None,
                        None,
                        Some(next.to_owned()),
                    );
                })
                .w(px(38.0))
                .h(px(30.0))
                .px(px(7.0))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .text_size(px(12.0))
                .tabular_nums()
                .text_color(palette::text_primary()),
        );

    if can_remove {
        let id_rm = loop_id;
        let cid_rm = cond_id;
        let rm_test_id = format!("wfe-exit-remove-{index}");
        row = row.child(
            button()
                .test_id(&rm_test_id)
                .accessibility_label("Remove condition")
                .on_press(move || store::remove_workflow_editor_exit_condition(&id_rm, &cid_rm))
                .w(px(22.0))
                .h(px(22.0))
                .rounded(px(5.0))
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(icons::close_x(palette::text_muted())),
        );
    }

    row
}

fn find_condition(loop_id: &str, cond_id: &str) -> Option<LoopExitCondition> {
    store::active_workflow_editor()
        .find_loop(loop_id)
        .and_then(|l| l.exit_conditions.iter().find(|c| c.id == cond_id).cloned())
}

fn exit_step_select(loop_id: String, cond_id: String, index: usize) -> impl IntoElement {
    let id_label = loop_id.clone();
    let cid_label = cond_id.clone();
    let id_items = loop_id.clone();
    let id_val = loop_id.clone();
    let cid_val = cond_id.clone();
    let id_chg = loop_id;
    let cid_chg = cond_id;
    let test_id = format!("wfe-exit-step-{index}");

    let trigger = view()
        .h(px(30.0))
        .px(px(7.5))
        .rounded(px(6.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(move || {
                let doc = store::active_workflow_editor();
                let Some(block) = doc.find_loop(&id_label) else {
                    return "Step".to_owned();
                };
                let step_id = block
                    .exit_conditions
                    .iter()
                    .find(|c| c.id == cid_label)
                    .map(|c| c.step_id.as_str())
                    .unwrap_or("");
                if let Some(step) = block.steps.iter().find(|s| s.id == step_id) {
                    let num = doc.step_number(&step.id).unwrap_or(1);
                    format!("{num} · {}", step.name)
                } else {
                    "Step".to_owned()
                }
            })
            .text_size(px(11.5))
            .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()));

    select()
        .test_id(&test_id)
        .accessibility_label("Exit condition step")
        .flex_1()
        .min_w(px(0.0))
        .items(move || {
            let doc = store::active_workflow_editor();
            let Some(block) = doc.find_loop(&id_items) else {
                return Vec::new();
            };
            block
                .steps
                .iter()
                .map(|step| {
                    let num = doc.step_number(&step.id).unwrap_or(1);
                    SelectItem::new(step.id.clone(), format!("{num} · {}", step.name))
                })
                .collect()
        })
        .value(move || {
            let sid = find_condition(&id_val, &cid_val)
                .map(|c| c.step_id)
                .unwrap_or_default();
            SelectValue::from(sid)
        })
        .on_change(move |value: &Option<SharedString>| {
            if let Some(val) = value.as_deref() {
                store::update_workflow_editor_exit_condition(
                    &id_chg,
                    &cid_chg,
                    Some(val.to_owned()),
                    None,
                    None,
                    None,
                );
            }
        })
        .cover(false)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
        .item_class("px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium")
        .trigger(trigger)
}

fn exit_field_select(loop_id: String, cond_id: String, index: usize) -> impl IntoElement {
    let id_label = loop_id.clone();
    let cid_label = cond_id.clone();
    let id_val = loop_id.clone();
    let cid_val = cond_id.clone();
    let id_chg = loop_id;
    let cid_chg = cond_id;
    let test_id = format!("wfe-exit-field-{index}");

    let trigger = view()
        .h(px(30.0))
        .px(px(7.0))
        .rounded(px(6.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(move || {
                find_condition(&id_label, &cid_label)
                    .map(|c| c.field.label())
                    .unwrap_or("exit code")
            })
            .text_size(px(11.5))
            .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()));

    select()
        .test_id(&test_id)
        .accessibility_label("Exit condition field")
        .items(|| {
            ExitField::ALL
                .iter()
                .map(|field| SelectItem::new(field.value(), field.label()))
                .collect::<Vec<_>>()
        })
        .value(move || {
            let val = find_condition(&id_val, &cid_val)
                .map(|c| c.field.value())
                .unwrap_or("exit_code");
            SelectValue::from(val)
        })
        .on_change(move |value: &Option<SharedString>| {
            if let Some(val) = value.as_deref() {
                store::update_workflow_editor_exit_condition(
                    &id_chg,
                    &cid_chg,
                    None,
                    Some(ExitField::from_value(val)),
                    None,
                    None,
                );
            }
        })
        .cover(false)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
        .item_class("px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium")
        .trigger(trigger)
}

fn exit_operator_select(loop_id: String, cond_id: String, index: usize) -> impl IntoElement {
    let id_label = loop_id.clone();
    let cid_label = cond_id.clone();
    let id_val = loop_id.clone();
    let cid_val = cond_id.clone();
    let id_chg = loop_id;
    let cid_chg = cond_id;
    let test_id = format!("wfe-exit-op-{index}");

    let trigger = view()
        .h(px(30.0))
        .px(px(6.5))
        .rounded(px(6.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(3.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(move || {
                find_condition(&id_label, &cid_label)
                    .map(|c| c.operator.symbol())
                    .unwrap_or("=")
            })
            .text_size(px(11.5))
            .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()));

    select()
        .test_id(&test_id)
        .accessibility_label("Exit condition operator")
        .items(|| {
            ExitOperator::ALL
                .iter()
                .map(|op| SelectItem::new(op.value(), op.symbol()))
                .collect::<Vec<_>>()
        })
        .value(move || {
            let val = find_condition(&id_val, &cid_val)
                .map(|c| c.operator.value())
                .unwrap_or("eq");
            SelectValue::from(val)
        })
        .on_change(move |value: &Option<SharedString>| {
            if let Some(val) = value.as_deref() {
                store::update_workflow_editor_exit_condition(
                    &id_chg,
                    &cid_chg,
                    None,
                    None,
                    Some(ExitOperator::from_value(val)),
                    None,
                );
            }
        })
        .cover(false)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
        .item_class("px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium")
        .trigger(trigger)
}

fn loop_wait_select(loop_id: String) -> impl IntoElement {
    let id_label = loop_id.clone();
    let id_val = loop_id.clone();
    let id_chg = loop_id;

    let trigger = view()
        .w_full()
        .h(px(32.0))
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .flex_row()
        .items_center()
        .justify_between()
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(move || {
                store::active_workflow_editor()
                    .find_loop(&id_label)
                    .map(|l| l.wait_between.label())
                    .unwrap_or("None")
            })
            .text_size(px(12.5))
            .text_color(palette::text_primary()),
        );

    select()
        .test_id("wfe-loop-wait-between")
        .accessibility_label("Wait between attempts")
        .w_full()
        .items(|| {
            LoopWaitBetween::ALL
                .iter()
                .map(|w| SelectItem::new(w.value(), w.label()))
                .collect::<Vec<_>>()
        })
        .value(move || {
            let val = store::active_workflow_editor()
                .find_loop(&id_val)
                .map(|l| l.wait_between.value())
                .unwrap_or("none");
            SelectValue::from(val)
        })
        .on_change(move |value: &Option<SharedString>| {
            if let Some(val) = value.as_deref() {
                store::set_workflow_editor_loop_wait(&id_chg, LoopWaitBetween::from_value(val));
            }
        })
        .cover(false)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
        .item_class("px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium")
        .trigger(trigger)
}

fn loop_between_attempts_section(
    loop_id_pass: String,
    loop_id_discard: String,
) -> impl IntoElement {
    let pass_check = loop_id_pass.clone();
    let discard_check = loop_id_discard.clone();

    view()
        .gap(px(9.0))
        .child(inspector_field_label("Between attempts"))
        .child(
            button()
                .test_id("wfe-loop-pass-prev")
                .accessibility_label("Pass the last attempt's outputs to the next one")
                .on_press(move || store::toggle_workflow_editor_loop_pass_previous(&loop_id_pass))
                .w_full()
                .flex_row()
                .items_start()
                .gap(px(8.0))
                .active(|s| s.opacity(0.85))
                .child(view().pt(px(1.5)).child(dynamic(move || {
                    let checked = store::active_workflow_editor()
                        .find_loop(&pass_check)
                        .is_some_and(|l| l.pass_previous_outputs);
                    checkbox_box(checked)
                })))
                .child(
                    view()
                        .flex_1()
                        .gap(px(2.0))
                        .child(
                            text("Pass the last attempt's outputs to the next one as")
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text("$loop.previous")
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .text_color(palette::text_secondary()),
                        ),
                ),
        )
        .child(
            button()
                .test_id("wfe-loop-discard-changes")
                .accessibility_label("Discard file changes before each attempt")
                .on_press(move || {
                    store::toggle_workflow_editor_loop_discard_changes(&loop_id_discard);
                })
                .w_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .active(|s| s.opacity(0.85))
                .child(dynamic(move || {
                    let checked = store::active_workflow_editor()
                        .find_loop(&discard_check)
                        .is_some_and(|l| l.discard_file_changes);
                    checkbox_box(checked)
                }))
                .child(
                    text("Discard file changes before each attempt")
                        .flex_1()
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                ),
        )
}

fn loop_on_exhausted_select(loop_id: String) -> impl IntoElement {
    let id_label = loop_id.clone();
    let id_items = loop_id.clone();
    let id_val = loop_id.clone();
    let id_chg = loop_id;

    let trigger = view()
        .w_full()
        .h(px(32.0))
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(move || {
                store::active_workflow_editor()
                    .find_loop(&id_label)
                    .map(|l| l.on_exhausted.label(&l.fallback.name))
                    .unwrap_or_else(|| "Go to step: Report failure".to_owned())
            })
            .text_size(px(12.5))
            .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()));

    select()
        .test_id("wfe-loop-on-exhausted")
        .accessibility_label("If attempts run out")
        .w_full()
        .items(move || {
            let fallback_name = store::active_workflow_editor()
                .find_loop(&id_items)
                .map(|l| l.fallback.name.clone())
                .unwrap_or_else(|| "Report failure".to_owned());
            LoopExhaustedAction::ALL
                .iter()
                .map(|action| SelectItem::new(action.value(), action.label(&fallback_name)))
                .collect::<Vec<_>>()
        })
        .value(move || {
            let val = store::active_workflow_editor()
                .find_loop(&id_val)
                .map(|l| l.on_exhausted.value())
                .unwrap_or("go_to_fallback");
            SelectValue::from(val)
        })
        .on_change(move |value: &Option<SharedString>| {
            if let Some(val) = value.as_deref() {
                store::set_workflow_editor_loop_on_exhausted(
                    &id_chg,
                    LoopExhaustedAction::from_value(val),
                );
            }
        })
        .cover(false)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
        .item_class("px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium")
        .trigger(trigger)
}

// --- Contextual Inspectors for Steps, Triggers, and Fallback ----------------------------

fn step_inspector(step_id: &str) -> impl IntoElement {
    let id_hdr = step_id.to_owned();
    let id_name_val = step_id.to_owned();
    let id_name_chg = step_id.to_owned();
    let id_actor = step_id.to_owned();
    let id_detail_lbl = step_id.to_owned();
    let id_detail_val = step_id.to_owned();
    let id_detail_chg = step_id.to_owned();
    let id_remove = step_id.to_owned();

    view()
        .w_full()
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(16.0))
                .py(px(12.0))
                .border_b_1()
                .border_color(palette::border_color())
                .child(
                    text(move || {
                        let doc = store::active_workflow_editor();
                        let num = doc.step_number(&id_hdr).unwrap_or(1);
                        let kind_label = doc
                            .find_step(&id_hdr)
                            .map(|s| s.kind.title())
                            .unwrap_or("Step");
                        format!("Step {num} · {kind_label}")
                    })
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .p(px(16.0))
                .gap(px(14.0))
                .child(
                    view()
                        .gap(px(6.0))
                        .child(inspector_field_label("Name"))
                        .child(
                            input()
                                .test_id("wfe-step-name")
                                .value(move || {
                                    store::active_workflow_editor()
                                        .find_step(&id_name_val)
                                        .map(|s| s.name.clone())
                                        .unwrap_or_default()
                                })
                                .on_change(move |next: &str| {
                                    store::update_workflow_editor_step_name(&id_name_chg, next);
                                })
                                .h(px(32.0))
                                .px(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .gap(px(6.0))
                        .child(inspector_field_label("Runner"))
                        .child(
                            view()
                                .h(px(32.0))
                                .px(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::divider_color())
                                .bg(palette::card_inset_bg())
                                .justify_center()
                                .child(
                                    text(move || {
                                        store::active_workflow_editor()
                                            .find_step(&id_actor)
                                            .map(|s| s.actor_badge.clone())
                                            .unwrap_or_default()
                                    })
                                    .text_size(px(12.5))
                                    .text_color(palette::text_secondary()),
                                ),
                        ),
                )
                .child(
                    view()
                        .gap(px(6.0))
                        .child(
                            text(move || {
                                let is_shell = store::active_workflow_editor()
                                    .find_step(&id_detail_lbl)
                                    .is_some_and(|s| s.kind == StepTypeKind::ShellCommand);
                                if is_shell { "Command" } else { "Instructions" }
                            })
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_muted()),
                        )
                        .child(
                            input()
                                .test_id("wfe-step-detail")
                                .value(move || {
                                    store::active_workflow_editor()
                                        .find_step(&id_detail_val)
                                        .map(|s| s.detail.clone())
                                        .unwrap_or_default()
                                })
                                .on_change(move |next: &str| {
                                    store::update_workflow_editor_step_detail(&id_detail_chg, next);
                                })
                                .h(px(32.0))
                                .px(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .pt(px(8.0))
                        .border_t_1()
                        .border_color(palette::divider_color())
                        .child(
                            button()
                                .test_id("wfe-step-remove")
                                .accessibility_label("Remove step")
                                .on_press(move || store::remove_workflow_editor_step(&id_remove))
                                .h(px(30.0))
                                .px(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .child(
                                    text("Remove step")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::destructive_text()),
                                ),
                        ),
                ),
        )
}

fn fallback_inspector(loop_id: &str) -> impl IntoElement {
    let id_name_val = loop_id.to_owned();
    let id_name_chg = loop_id.to_owned();
    let id_meta = loop_id.to_owned();
    let id_back = loop_id.to_owned();

    view()
        .w_full()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .px(px(16.0))
                .py(px(12.0))
                .border_b_1()
                .border_color(palette::border_color())
                .child(icons::alert_circle(palette::editor_unsaved_text()))
                .child(
                    text("Fallback · If out of attempts")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .p(px(16.0))
                .gap(px(14.0))
                .child(
                    view()
                        .gap(px(6.0))
                        .child(inspector_field_label("Fallback step name"))
                        .child(
                            input()
                                .test_id("wfe-fallback-name")
                                .value(move || {
                                    store::active_workflow_editor()
                                        .find_loop(&id_name_val)
                                        .map(|l| l.fallback.name.clone())
                                        .unwrap_or_default()
                                })
                                .on_change(move |next: &str| {
                                    store::update_workflow_editor_fallback_name(&id_name_chg, next);
                                })
                                .h(px(32.0))
                                .px(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .gap(px(6.0))
                        .child(inspector_field_label("Behavior"))
                        .child(
                            view()
                                .p(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::divider_color())
                                .bg(palette::card_inset_bg())
                                .child(
                                    text(move || {
                                        store::active_workflow_editor()
                                            .find_loop(&id_meta)
                                            .map(|l| {
                                                format!(
                                                    "{} · {}",
                                                    l.fallback.action_label,
                                                    l.fallback.outcome_label
                                                )
                                            })
                                            .unwrap_or_default()
                                    })
                                    .text_size(px(12.0))
                                    .text_color(palette::text_secondary()),
                                ),
                        ),
                )
                .child(
                    button()
                        .on_press(move || {
                            store::select_workflow_editor_node(EditorSelection::Loop(
                                id_back.clone(),
                            ));
                        })
                        .h(px(30.0))
                        .px(px(10.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Back to Loop settings")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
}

fn trigger_inspector() -> impl IntoElement {
    view()
        .w_full()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .px(px(16.0))
                .py(px(12.0))
                .border_b_1()
                .border_color(palette::border_color())
                .child(icons::work_queue(palette::text_secondary()))
                .child(
                    text("Starts from · Workflow triggers")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .p(px(16.0))
                .gap(px(14.0))
                .child(
                    view()
                        .gap(px(10.0))
                        .child(inspector_field_label("Entry points"))
                        .child(
                            button()
                                .test_id("wfe-trigger-work-queue")
                                .accessibility_label("Work queue intake rules")
                                .on_press(store::toggle_workflow_editor_trigger_work_queue)
                                .w_full()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(dynamic(|| {
                                    checkbox_box(
                                        store::active_workflow_editor().trigger.work_queue_enabled,
                                    )
                                }))
                                .child(
                                    text("Work queue · intake rules")
                                        .text_size(px(12.5))
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("wfe-trigger-ad-hoc")
                                .accessibility_label("Allow ad-hoc runs")
                                .on_press(store::toggle_workflow_editor_trigger_ad_hoc)
                                .w_full()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(dynamic(|| {
                                    checkbox_box(
                                        store::active_workflow_editor().trigger.ad_hoc_allowed,
                                    )
                                }))
                                .child(
                                    text("Ad-hoc runs allowed")
                                        .text_size(px(12.5))
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                )
                .child(
                    view()
                        .gap(px(6.0))
                        .child(inspector_field_label("Inputs"))
                        .child(
                            view()
                                .p(px(10.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::divider_color())
                                .bg(palette::card_inset_bg())
                                .child(
                                    text(move || {
                                        store::active_workflow_editor().trigger.inputs_summary
                                    })
                                    .app_class("font-mono")
                                    .text_size(px(11.5))
                                    .text_color(palette::text_secondary()),
                                ),
                        ),
                ),
        )
}

// --- Shared Inspector Primitives --------------------------------------------------------

fn inspector_field_label(label: &'static str) -> impl IntoElement {
    text(label)
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::text_muted())
}

fn radio_dot(selected: bool) -> impl IntoElement {
    let outer = view()
        .w(px(15.0))
        .h(px(15.0))
        .rounded_full()
        .flex_shrink_0()
        .items_center()
        .justify_center();

    if selected {
        outer.bg(palette::nav_active_bg()).child(
            view()
                .w(px(5.5))
                .h(px(5.5))
                .rounded_full()
                .bg(palette::nav_active_fg()),
        )
    } else {
        outer
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::nav_active_fg())
    }
}

fn checkbox_box(checked: bool) -> impl IntoElement {
    view()
        .w(px(15.5))
        .h(px(15.5))
        .rounded(px(4.0))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if checked {
            palette::nav_active_bg()
        } else {
            palette::border_color()
        })
        .bg(if checked {
            palette::nav_active_bg()
        } else {
            palette::nav_active_fg()
        })
        .child(when(
            move || checked,
            || icons::check(palette::nav_active_fg()),
        ))
}
