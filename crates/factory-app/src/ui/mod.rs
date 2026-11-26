//! Main macOS desktop shell: unified header toolbar, left navigation rail, and active canvas.

mod chat;
mod claude_spawn_settings;
mod codex_spawn_settings;
mod dashboard;
pub(crate) mod home_setup;
mod icons;
pub(crate) mod inbox;
mod machines;
mod mcp;
pub(crate) mod menubar;
mod palette;
pub(crate) mod preferences;
mod project_settings;
mod register_project;
pub(crate) mod secrets;
mod work_queue;
mod workflow_editor;
mod workflow_run;
mod workflows;
mod workspace;

use factory_core::{
    DashboardGranularity, PreferencesTab, ProjectColor, ProjectId, RunExecutionState,
    WorkQueueViewMode,
};
use nsh_base::{SelectItem, SelectValue, button, select};
use nsh_core::prelude::*;

use crate::store::{self, ActiveView};

const ALL_PROJECTS_VALUE: &str = "__all_projects__";

/// Top-level macOS shell matching the reference Dashboard layout.
pub(crate) fn shell() -> impl IntoElement {
    static FIRST_BUILD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let _boot_span = crate::boot_log::once_span("boot.ui.shell_first_build", &FIRST_BUILD);
    view()
        .flex_1()
        .bg(palette::canvas_bg())
        .child(when(
            || store::active_view() == ActiveView::WorkflowEditor,
            workflow_editor::workflow_editor_header,
        ))
        .child(when(
            || store::active_view() != ActiveView::WorkflowEditor,
            top_header_bar,
        ))
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .flex_row()
                .child(when(
                    || {
                        store::sidebar_visible()
                            && store::active_view() != ActiveView::WorkflowEditor
                    },
                    sidebar,
                ))
                .child(content_pane()),
        )
        .child(when(
            || {
                store::active_view() == ActiveView::WorkflowRun
                    && !store::active_workflow_run().is_empty()
            },
            workflow_run_footer_bar,
        ))
}

fn top_header_bar() -> impl IntoElement {
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
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    view().window_drag(false).child(
                        button()
                            .accessibility_label("Toggle Sidebar")
                            .on_press(store::toggle_sidebar)
                            .w(px(26.0))
                            .h(px(26.0))
                            .rounded(px(6.0))
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(palette::sidebar_hover_bg()))
                            .active(|s| s.opacity(0.75))
                            .child(icons::sidebar_toggle(palette::text_secondary())),
                    ),
                )
                .child(header_title_block()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .child(when(
                    || store::active_view() == ActiveView::Dashboard,
                    dashboard_header_controls,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Inbox,
                    inbox::inbox_header_controls,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Machines,
                    machines::machines_header_controls,
                ))
                .child(when(
                    || store::active_view() == ActiveView::WorkQueue,
                    work_queue_header_controls,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Workflows,
                    workflows::workflows_header_controls,
                ))
                .child(when(
                    || store::active_view() == ActiveView::WorkflowRun,
                    workflow_run_header_controls,
                )),
        )
}

fn header_title_block() -> impl IntoElement {
    view()
        .gap(px(1.0))
        .child(
            text(move || match store::active_view() {
                ActiveView::Dashboard => "Dashboard".to_owned(),
                ActiveView::Inbox => "Inbox".to_owned(),
                ActiveView::Machines => "Machines".to_owned(),
                ActiveView::WorkQueue => "Work queue".to_owned(),
                ActiveView::Workflows => "Workflows".to_owned(),
                ActiveView::WorkflowEditor => store::active_workflow_editor().workflow_name,
                ActiveView::WorkflowRun => store::active_workflow_run().workflow_name,
                ActiveView::ProjectSettings => "Project settings".to_owned(),
                ActiveView::RegisterProject => "Register Project".to_owned(),
                ActiveView::Chat => store::active_chat_meta()
                    .map(|chat| chat.title)
                    .unwrap_or_else(|| "New chat".to_owned()),
                ActiveView::Workspace => store::selected_project()
                    .map(|p| p.name)
                    .unwrap_or_else(|| "Workspace".to_owned()),
            })
            .text_size(px(14.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_primary()),
        )
        .child(
            text(move || match store::active_view() {
                ActiveView::Dashboard => {
                    let snap = store::dashboard_snapshot();
                    if snap.is_empty() {
                        "No activity yet".to_owned()
                    } else {
                        format!("{} · {}", snap.filter_label, snap.date_range_label)
                    }
                }
                ActiveView::Inbox => store::inbox_state().header_subtitle(),
                ActiveView::Machines => store::machines_fleet().header_subtitle(),
                ActiveView::WorkQueue => {
                    let queue = store::active_work_queue();
                    if queue.project_name.is_empty() {
                        "No project selected".to_owned()
                    } else if queue.repo_slug.is_empty() {
                        format!("{} · No issue source connected", queue.project_name)
                    } else {
                        format!(
                            "{} · {} · {}",
                            queue.project_name, queue.tracker_name, queue.repo_slug
                        )
                    }
                }
                ActiveView::Workflows => {
                    let overview = store::active_workflows_overview();
                    if overview.project_name.is_empty() {
                        "No project selected".to_owned()
                    } else {
                        overview.header_subtitle()
                    }
                }
                ActiveView::WorkflowEditor => {
                    let doc = store::active_workflow_editor();
                    format!("{} › Workflows", doc.project_name)
                }
                ActiveView::WorkflowRun => {
                    let run = store::active_workflow_run();
                    if run.is_empty() {
                        format!("{} · No runs yet", run.project_name)
                    } else {
                        format!("{} · Run #{}", run.project_name, run.run_number)
                    }
                }
                ActiveView::ProjectSettings => {
                    let settings = store::active_project_settings();
                    if settings.project_name.is_empty() {
                        "No project selected".to_owned()
                    } else {
                        format!(
                            "{} · {}",
                            settings.project_name,
                            store::project_settings_tab().label()
                        )
                    }
                }
                ActiveView::RegisterProject => {
                    "Clone into your Bruk folder or link an existing clone".to_owned()
                }
                ActiveView::Chat => match store::selected_project() {
                    Some(project) => format!("{} · Chats", project.name),
                    None => "No project selected".to_owned(),
                },
                ActiveView::Workspace => store::selected_project()
                    .map(|p| format!("{} · {}", p.default_branch, p.root_path.display()))
                    .unwrap_or_else(|| "Select a project to run workflows".to_owned()),
            })
            .text_size(px(11.5))
            .text_color(palette::text_muted()),
        )
}

fn workflow_run_header_controls() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .window_drag(false)
        .child(
            button()
                .test_id("wf-header-search")
                .accessibility_label("Search or run a command")
                .on_press(store::toggle_workflow_run_command_palette)
                .h(px(30.0))
                .w(px(236.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.85))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(7.0))
                        .child(icons::search(palette::text_muted()))
                        .child(
                            text("Search or run a command")
                                .text_size(px(12.0))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    view()
                        .px(px(5.0))
                        .py(px(1.0))
                        .rounded(px(4.5))
                        .bg(palette::subtle_surface_bg())
                        .border_1()
                        .border_color(palette::divider_color())
                        .child(
                            text("⌘K")
                                .text_size(px(10.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .h(px(30.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .overflow_hidden()
                .child(
                    button()
                        .test_id("wf-header-pause")
                        .accessibility_label(move || {
                            Some(SharedString::from(
                                if store::active_workflow_run().execution_state
                                    == RunExecutionState::Paused
                                {
                                    "Resume Run"
                                } else {
                                    "Pause Run"
                                },
                            ))
                        })
                        .disabled(|| {
                            !matches!(
                                store::active_workflow_run().execution_state,
                                RunExecutionState::Running | RunExecutionState::Paused
                            )
                        })
                        .on_press(store::toggle_pause_workflow_run)
                        .w(px(32.0))
                        .h_full()
                        .items_center()
                        .justify_center()
                        .border_r_1()
                        .border_color(palette::divider_color())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.75))
                        .disabled_style(|s| s.opacity(0.35))
                        .child(dynamic(|| {
                            if store::active_workflow_run().execution_state
                                == RunExecutionState::Paused
                            {
                                icons::play_filled(palette::text_primary()).into_element()
                            } else {
                                icons::pause(palette::text_primary()).into_element()
                            }
                        })),
                )
                .child(
                    button()
                        .test_id("wf-header-stop")
                        .accessibility_label("Stop Run")
                        .disabled(|| store::active_workflow_run().execution_state.is_terminal())
                        .on_press(store::stop_workflow_run)
                        .w(px(32.0))
                        .h_full()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.75))
                        .disabled_style(|s| s.opacity(0.35))
                        .child(icons::stop_square(palette::text_primary())),
                ),
        )
        .child(
            button()
                .test_id("wf-header-edit")
                .accessibility_label("Edit Workflow")
                .on_press(store::edit_active_workflow)
                .h(px(30.0))
                .px(px(11.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .flex_row()
                .items_center()
                .gap(px(6.5))
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(icons::pencil(palette::text_secondary()))
                .child(
                    text("Edit Workflow")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
}

fn work_queue_header_controls() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .window_drag(false)
        .child(work_queue_mode_segmented_control())
        .child(
            button()
                .test_id("wq-header-sync")
                .accessibility_label("Sync")
                .disabled(store::work_queue_syncing)
                .on_press(store::sync_work_queue)
                .h(px(30.0))
                .px(px(11.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .disabled_style(|s| s.opacity(0.55))
                .child(icons::sync_arrows(palette::text_secondary()))
                .child(
                    text(|| {
                        if store::work_queue_syncing() {
                            "Syncing…".to_owned()
                        } else {
                            "Sync".to_owned()
                        }
                    })
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .test_id("wq-header-adhoc")
                .accessibility_label("Ad-hoc Run...")
                .on_press(|| {
                    let open = store::work_queue_ad_hoc_open();
                    store::set_work_queue_ad_hoc_open(!open);
                })
                .h(px(30.0))
                .px(px(12.0))
                .rounded(px(8.0))
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

fn work_queue_mode_segmented_control() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .h(px(30.0))
        .p(px(2.5))
        .rounded(px(8.0))
        .bg(palette::segmented_track_bg())
        .gap(px(2.0))
        .child(work_queue_mode_segment(WorkQueueViewMode::Board))
        .child(work_queue_mode_segment(WorkQueueViewMode::List))
}

fn work_queue_mode_segment(mode: WorkQueueViewMode) -> impl IntoElement {
    let is_active = move || store::work_queue_view_mode() == mode;

    button()
        .on_press(move || store::set_work_queue_view_mode(mode))
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
            text(mode.label())
                .text_size(px(12.5))
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

fn dashboard_header_controls() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .window_drag(false)
        .child(period_navigator())
        .child(granularity_segmented_control())
        .child(project_filter_dropdown())
}

fn period_navigator() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .h(px(30.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .overflow_hidden()
        .child(
            button()
                .accessibility_label("Previous period")
                .disabled(move || !store::dashboard_snapshot().can_step_prev)
                .on_press(|| store::step_dashboard_period(-1))
                .w(px(28.0))
                .h_full()
                .items_center()
                .justify_center()
                .border_r_1()
                .border_color(palette::divider_color())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.75))
                .disabled_style(|s| s.opacity(0.35))
                .child(icons::chevron_left(palette::text_secondary())),
        )
        .child(
            view()
                .px(px(12.0))
                .h_full()
                .items_center()
                .justify_center()
                .child(
                    text(move || store::dashboard_snapshot().period_label)
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .accessibility_label("Next period")
                .disabled(move || !store::dashboard_snapshot().can_step_next)
                .on_press(|| store::step_dashboard_period(1))
                .w(px(28.0))
                .h_full()
                .items_center()
                .justify_center()
                .border_l_1()
                .border_color(palette::divider_color())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.75))
                .disabled_style(|s| s.opacity(0.35))
                .child(icons::chevron_right(palette::text_secondary())),
        )
}

fn granularity_segmented_control() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .h(px(30.0))
        .p(px(2.5))
        .rounded(px(8.0))
        .bg(palette::segmented_track_bg())
        .gap(px(2.0))
        .child(granularity_segment(DashboardGranularity::Week))
        .child(granularity_segment(DashboardGranularity::Month))
}

fn granularity_segment(granularity: DashboardGranularity) -> impl IntoElement {
    let is_active = move || store::dashboard_granularity() == granularity;

    button()
        .on_press(move || store::set_dashboard_granularity(granularity))
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
            text(granularity.label())
                .text_size(px(12.5))
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

fn project_filter_dropdown() -> impl IntoElement {
    let trigger = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .h(px(30.0))
        .min_w(px(120.0))
        .px(px(11.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(move || {
                store::dashboard_project_filter().unwrap_or_else(|| "All projects".to_owned())
            })
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()));

    select()
        .accessibility_label("Project filter")
        .items(move || {
            let mut items = vec![SelectItem::new(ALL_PROJECTS_VALUE, "All projects")];
            for name in store::dashboard_filter_options() {
                items.push(SelectItem::new(name.clone(), name));
            }
            items
        })
        .value(move || {
            let selected = store::dashboard_project_filter()
                .unwrap_or_else(|| ALL_PROJECTS_VALUE.to_owned());
            SelectValue::from(selected)
        })
        .on_change(|value: &Option<SharedString>| match value.as_deref() {
            None | Some(ALL_PROJECTS_VALUE) => store::set_dashboard_project_filter(None),
            Some(project) => store::set_dashboard_project_filter(Some(project.to_owned())),
        })
        .cover(false)
        .placement(Placement::Bottom)
        .align(Align::End)
        .offset(px(4.0))
        .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
        .item_class("px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium")
        .trigger(trigger)
}

// --- Left Sidebar -----------------------------------------------------------------------

fn sidebar() -> impl IntoElement {
    view()
        .w(move || {
            if store::preferences().appearance.compact_sidebar {
                px(188.0)
            } else {
                px(210.0)
            }
        })
        .flex_shrink_0()
        .bg(palette::sidebar_bg())
        .border_r_1()
        .border_color(palette::border_color())
        .px(px(10.0))
        .py(px(12.0))
        .gap(px(16.0))
        .child(
            view()
                .gap(px(2.0))
                .child(sidebar_dashboard_item())
                .child(sidebar_inbox_item())
                .child(sidebar_machines_item()),
        )
        .child(
            scroll().flex_1().child(
                view()
                    .gap(px(4.0))
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(8.0))
                            .pb(px(2.0))
                            .child(
                                text("Projects")
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_muted()),
                            )
                            .child(
                                button()
                                    .accessibility_label("Register Project")
                                    .on_press(store::open_register_project)
                                    .w(px(22.0))
                                    .h(px(22.0))
                                    .rounded(px(6.0))
                                    .items_center()
                                    .justify_center()
                                    .bg(move || {
                                        if store::active_view() == ActiveView::RegisterProject {
                                            palette::nav_active_bg()
                                        } else {
                                            AdaptiveColor::TRANSPARENT
                                        }
                                    })
                                    .hover(|s| s.bg(palette::sidebar_hover_bg()))
                                    .active(|s| s.opacity(0.75))
                                    .child(icons::plus(move || {
                                        if store::active_view() == ActiveView::RegisterProject {
                                            palette::nav_active_fg()
                                        } else {
                                            palette::text_muted()
                                        }
                                    })),
                            ),
                    )
                    .child(when(
                        || store::project_ids().is_empty(),
                        || {
                            view()
                                .mx(px(4.0))
                                .mt(px(4.0))
                                .p(px(10.0))
                                .rounded(px(8.0))
                                .border_1()
                                .border_color(palette::divider_color())
                                .bg(palette::card_bg())
                                .gap(px(6.0))
                                .child(
                                    text("No projects yet")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_secondary()),
                                )
                                .child(
                                    text("Register a local repository to get started.")
                                        .text_size(px(11.0))
                                        .text_color(palette::text_muted()),
                                )
                                .child(
                                    button()
                                        .test_id("sidebar-empty-register-project")
                                        .accessibility_label("Register a Project")
                                        .on_press(store::open_register_project)
                                        .mt(px(2.0))
                                        .px(px(8.0))
                                        .py(px(4.5))
                                        .rounded(px(6.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::control_bg())
                                        .items_center()
                                        .justify_center()
                                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                                        .active(|s| s.opacity(0.8))
                                        .child(
                                            text("Register Project")
                                                .text_size(px(11.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(palette::text_primary()),
                                        ),
                                )
                        },
                    ))
                    .child(each(
                        store::project_ids,
                        |id: &ProjectId| *id,
                        |id, _| sidebar_project_row(id.get_untracked()),
                    ))
                    .child(when(
                        || {
                            store::active_view() == ActiveView::WorkflowRun
                                && !store::deploy_watch_entries().is_empty()
                        },
                        sidebar_deploy_watch_section,
                    )),
            ),
        )
        .child(when(
            || store::active_view() == ActiveView::WorkQueue,
            || {
                view()
                    .pt(px(8.0))
                    .border_t_1()
                    .border_color(palette::divider_color())
                    .child(sidebar_agent_slots_summary())
            },
        ))
}

fn sidebar_deploy_watch_section() -> impl IntoElement {
    view()
        .mt(px(12.0))
        .pt(px(10.0))
        .px(px(6.0))
        .gap(px(6.0))
        .border_t_1()
        .border_color(palette::divider_color())
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
                        .gap(px(5.0))
                        .child(icons::extension_puzzle(12.0, palette::text_muted()))
                        .child(
                            text("Deploy Watch")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    view()
                        .px(px(5.5))
                        .py(px(1.0))
                        .rounded(px(4.5))
                        .bg(palette::subtle_surface_bg())
                        .border_1()
                        .border_color(palette::divider_color())
                        .child(
                            text("Extension")
                                .text_size(px(9.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(dynamic(|| {
            let mut rows = view().gap(px(4.0));
            for entry in store::deploy_watch_entries() {
                rows = rows.child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px(px(2.0))
                        .child(
                            text(entry.environment)
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(entry.version)
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .tabular_nums()
                                .text_color(palette::text_muted()),
                        ),
                );
            }
            rows
        }))
}

fn sidebar_agent_slots_summary() -> impl IntoElement {
    view()
        .px(px(10.0))
        .gap(px(1.5))
        .child(
            text("Agent slots")
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
        .child(
            text(move || store::active_work_queue().agent_slots_summary())
                .text_size(px(11.5))
                .tabular_nums()
                .text_color(palette::text_muted()),
        )
}

fn sidebar_dashboard_item() -> impl IntoElement {
    let is_active = || store::active_view() == ActiveView::Dashboard;
    button()
        .test_id("sidebar-dashboard-btn")
        .accessibility_label("Dashboard")
        .on_press(store::open_dashboard)
        .rounded(px(7.0))
        .hover(|s| s.bg(palette::sidebar_hover_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(9.0))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(7.0))
                .bg(move || {
                    if is_active() {
                        palette::nav_active_bg()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .child(icons::dashboard(move || {
                    if is_active() {
                        palette::nav_active_fg()
                    } else {
                        palette::text_secondary()
                    }
                }))
                .child(
                    text("Dashboard")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || {
                            if is_active() {
                                palette::nav_active_fg()
                            } else {
                                palette::text_primary()
                            }
                        }),
                ),
        )
}

fn sidebar_inbox_item() -> impl IntoElement {
    let is_active = || store::active_view() == ActiveView::Inbox;
    button()
        .test_id("sidebar-inbox-btn")
        .on_press(store::open_inbox)
        .rounded(px(7.0))
        .hover(|s| s.bg(palette::sidebar_hover_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(7.0))
                .bg(move || {
                    if is_active() {
                        palette::nav_active_bg()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(9.0))
                        .child(icons::inbox(move || {
                            if is_active() {
                                palette::nav_active_fg()
                            } else {
                                palette::text_secondary()
                            }
                        }))
                        .child(
                            text("Inbox")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(move || {
                                    if is_active() {
                                        palette::nav_active_fg()
                                    } else {
                                        palette::text_primary()
                                    }
                                }),
                        ),
                )
                .child(when(
                    || store::pending_inbox_count() > 0,
                    move || {
                        view()
                            .min_w(px(22.0))
                            .px(px(7.0))
                            .py(px(1.5))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(move || {
                                if is_active() {
                                    AdaptiveColor::new(rgba(0xFFFFFF38), rgba(0xFFFFFF38))
                                } else {
                                    palette::badge_orange_bg()
                                }
                            })
                            .child(
                                text(move || store::pending_inbox_count().to_string())
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .tabular_nums()
                                    .text_color(palette::nav_active_fg()),
                            )
                    },
                )),
        )
}

fn sidebar_machines_item() -> impl IntoElement {
    let is_active = || store::active_view() == ActiveView::Machines;
    button()
        .test_id("sidebar-machines-btn")
        .accessibility_label(move || {
            let count = store::machines().len();
            Some(SharedString::from(format!("Machines, {count} registered")))
        })
        .on_press(store::open_machines)
        .rounded(px(7.0))
        .hover(|s| s.bg(palette::sidebar_hover_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(6.0))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(7.0))
                .bg(move || {
                    if is_active() {
                        palette::nav_active_bg()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(9.0))
                        .child(icons::machines(move || {
                            if is_active() {
                                palette::nav_active_fg()
                            } else {
                                palette::text_secondary()
                            }
                        }))
                        .child(
                            text("Machines")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(move || {
                                    if is_active() {
                                        palette::nav_active_fg()
                                    } else {
                                        palette::text_primary()
                                    }
                                }),
                        ),
                )
                .child(
                    text(move || store::machines().len().to_string())
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .tabular_nums()
                        .text_color(move || {
                            if is_active() {
                                palette::nav_active_fg()
                            } else {
                                palette::text_muted()
                            }
                        }),
                ),
        )
}

fn sidebar_project_row(id: ProjectId) -> impl IntoElement {
    let is_selected = move || {
        store::active_view() == ActiveView::Workspace && store::selected_project_id() == Some(id)
    };
    let is_expanded = move || store::is_project_expanded(id);

    view()
        .gap(px(2.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(2.0))
                .child(
                    button()
                        .accessibility_label(move || {
                            let name = store::project_by_id(id).map(|p| p.name).unwrap_or_default();
                            Some(SharedString::from(if is_expanded() {
                                format!("Collapse {name}")
                            } else {
                                format!("Expand {name}")
                            }))
                        })
                        .on_press(move || store::toggle_project_expanded(id))
                        .w(px(20.0))
                        .h(px(24.0))
                        .rounded(px(5.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::sidebar_hover_bg()))
                        .child(dynamic(move || {
                            if is_expanded() {
                                icons::chevron_down(palette::text_muted()).into_element()
                            } else {
                                icons::chevron_right(palette::text_muted()).into_element()
                            }
                        })),
                )
                .child(
                    button()
                        .flex_1()
                        .min_w(px(0.0))
                        .on_press(move || store::select_project(id))
                        .rounded(px(7.0))
                        .hover(|s| s.bg(palette::sidebar_hover_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(7.5))
                                .px(px(6.0))
                                .py(px(5.5))
                                .rounded(px(7.0))
                                .bg(move || {
                                    if is_selected() {
                                        palette::nav_active_bg()
                                    } else {
                                        AdaptiveColor::TRANSPARENT
                                    }
                                })
                                .child(icons::folder(move || {
                                    if is_selected() {
                                        palette::nav_active_fg()
                                    } else {
                                        let color = store::project_by_id(id)
                                            .map(|p| ProjectColor::for_project_name(&p.name))
                                            .unwrap_or(ProjectColor::Blue);
                                        palette::project_color(color)
                                    }
                                }))
                                .child(
                                    text(move || {
                                        store::project_by_id(id).map(|p| p.name).unwrap_or_default()
                                    })
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(move || {
                                        if is_selected() {
                                            palette::nav_active_fg()
                                        } else {
                                            palette::text_primary()
                                        }
                                    }),
                                ),
                        ),
                ),
        )
        .child(when(is_expanded, move || sidebar_project_children(id)))
}

fn sidebar_project_children(id: ProjectId) -> impl IntoElement {
    let is_work_queue_active = move || {
        store::active_view() == ActiveView::WorkQueue && store::selected_project_id() == Some(id)
    };

    let work_queue_test_id = format!("sidebar-work-queue-{id}");

    view()
        .pl(px(10.0))
        .pb(px(4.0))
        .gap(px(3.0))
        .child(
            button()
                .test_id(&work_queue_test_id)
                .accessibility_label(move || {
                    let name = store::project_by_id(id).map(|p| p.name).unwrap_or_default();
                    Some(SharedString::from(format!("Work queue for {name}")))
                })
                .on_press(move || store::open_work_queue_for_project(id))
                .rounded(px(7.0))
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.85))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .px(px(9.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .bg(move || {
                            if is_work_queue_active() {
                                palette::nav_active_bg()
                            } else {
                                AdaptiveColor::TRANSPARENT
                            }
                        })
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(icons::work_queue(move || {
                                    if is_work_queue_active() {
                                        palette::nav_active_fg()
                                    } else {
                                        palette::text_secondary()
                                    }
                                }))
                                .child(
                                    text("Work queue")
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || {
                                            if is_work_queue_active() {
                                                palette::nav_active_fg()
                                            } else {
                                                palette::text_primary()
                                            }
                                        }),
                                ),
                        )
                        .child(
                            text(move || {
                                let q = store::work_queue_for_project(id);
                                let tracker = q.tracker_short_label();
                                if tracker.is_empty() {
                                    q.active_queue_count().to_string()
                                } else {
                                    format!("{tracker} · {}", q.active_queue_count())
                                }
                            })
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .tabular_nums()
                            .text_color(move || {
                                if is_work_queue_active() {
                                    palette::nav_active_fg()
                                } else {
                                    palette::text_muted()
                                }
                            }),
                        ),
                ),
        )
        .child({
            let is_workflows_active = move || {
                store::active_view() == ActiveView::Workflows
                    && store::selected_project_id() == Some(id)
            };
            let workflows_test_id = format!("sidebar-workflows-{id}");

            button()
                .test_id(&workflows_test_id)
                .accessibility_label(move || {
                    let name = store::project_by_id(id).map(|p| p.name).unwrap_or_default();
                    Some(SharedString::from(format!("Workflows for {name}")))
                })
                .on_press(move || store::open_workflows_for_project(id))
                .rounded(px(7.0))
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.85))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .px(px(9.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .bg(move || {
                            if is_workflows_active() {
                                palette::nav_active_bg()
                            } else {
                                AdaptiveColor::TRANSPARENT
                            }
                        })
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(icons::workflow_branch(14.0, move || {
                                    if is_workflows_active() {
                                        palette::nav_active_fg()
                                    } else {
                                        palette::text_secondary()
                                    }
                                }))
                                .child(
                                    text("Workflows")
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || {
                                            if is_workflows_active() {
                                                palette::nav_active_fg()
                                            } else {
                                                palette::text_primary()
                                            }
                                        }),
                                ),
                        )
                        .child(
                            text(move || {
                                store::workflows_overview_for_project(id)
                                    .workflows
                                    .len()
                                    .to_string()
                            })
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .tabular_nums()
                            .text_color(move || {
                                if is_workflows_active() {
                                    palette::nav_active_fg()
                                } else {
                                    palette::text_muted()
                                }
                            }),
                        ),
                )
        })
        .child(dynamic(move || {
            let queue = store::work_queue_for_project(id);
            let active_view = store::active_view();
            let active_key = store::active_workflow_run_key();
            let active_run = store::active_workflow_run();
            let mut list = view().gap(px(1.5));
            for wf in &queue.workflows {
                let wf_name = wf.name.clone();
                let wf_click = wf_name.clone();
                let slug = wf_name
                    .to_ascii_lowercase()
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "-");
                let wf_test_id = format!("sidebar-workflow-{id}-{slug}");
                let is_active = active_view == ActiveView::WorkflowRun
                    && active_key.0 == queue.project_name
                    && active_key.1 == wf_name;
                let running = queue.running_count_for_workflow(&wf.name);
                let show_running_ring = is_active
                    && matches!(
                        active_run.execution_state,
                        RunExecutionState::Running | RunExecutionState::Paused
                    );

                let trailing: AnyElement = if show_running_ring {
                    icons::status_running(palette::nav_active_fg()).into_element()
                } else if wf.has_failed_run {
                    text("Failed")
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if is_active {
                            palette::nav_active_fg()
                        } else {
                            palette::destructive_text()
                        })
                        .into_element()
                } else if running > 0 {
                    text(format!("{running} running"))
                        .text_size(px(10.5))
                        .tabular_nums()
                        .text_color(if is_active {
                            palette::nav_active_fg()
                        } else {
                            palette::text_muted()
                        })
                        .into_element()
                } else {
                    view().into_element()
                };

                list = list.child(
                    button()
                        .test_id(&wf_test_id)
                        .accessibility_label(format!("Workflow {wf_name}"))
                        .on_press(move || store::open_project_workflow(id, &wf_click))
                        .rounded(px(6.5))
                        .hover(move |s| {
                            if is_active {
                                s
                            } else {
                                s.bg(palette::sidebar_hover_bg())
                            }
                        })
                        .active(|s| s.opacity(0.85))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(6.0))
                                .px(px(9.0))
                                .py(px(5.0))
                                .rounded(px(6.5))
                                .bg(if is_active {
                                    palette::nav_active_bg()
                                } else {
                                    AdaptiveColor::TRANSPARENT
                                })
                                .child(
                                    view()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .flex_row()
                                        .items_center()
                                        .gap(px(7.0))
                                        .child(icons::workflow_branch(
                                            12.5,
                                            if is_active {
                                                palette::nav_active_fg()
                                            } else {
                                                palette::text_muted()
                                            },
                                        ))
                                        .child(
                                            text(wf_name)
                                                .flex_1()
                                                .text_size(px(12.0))
                                                .font_weight(if is_active {
                                                    FontWeight::MEDIUM
                                                } else {
                                                    FontWeight::NORMAL
                                                })
                                                .text_color(if is_active {
                                                    palette::nav_active_fg()
                                                } else {
                                                    palette::text_primary()
                                                }),
                                        ),
                                )
                                .child(trailing),
                        ),
                );
            }
            list
        }))
        .child({
            let is_chats_active = move || {
                store::active_view() == ActiveView::Chat && store::selected_project_id() == Some(id)
            };
            let chats_test_id = format!("sidebar-chats-{id}");
            button()
                .test_id(&chats_test_id)
                .accessibility_label(move || {
                    let name = store::project_by_id(id).map(|p| p.name).unwrap_or_default();
                    Some(SharedString::from(format!("Chats for {name}")))
                })
                .on_press(move || store::open_chats_for_project(id))
                .rounded(px(7.0))
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.85))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .px(px(9.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .bg(move || {
                            if is_chats_active() {
                                palette::nav_active_bg()
                            } else {
                                AdaptiveColor::TRANSPARENT
                            }
                        })
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(icons::chat_bubbles(14.0, move || {
                                    if is_chats_active() {
                                        palette::nav_active_fg()
                                    } else {
                                        palette::text_secondary()
                                    }
                                }))
                                .child(
                                    text("Chats")
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || {
                                            if is_chats_active() {
                                                palette::nav_active_fg()
                                            } else {
                                                palette::text_primary()
                                            }
                                        }),
                                ),
                        )
                        .child(dynamic(move || {
                            let (total, running) = store::chat_counts_for_project(id);
                            let color = if is_chats_active() {
                                palette::nav_active_fg()
                            } else {
                                palette::text_muted()
                            };
                            if running > 0 {
                                icons::status_running(if is_chats_active() {
                                    palette::nav_active_fg()
                                } else {
                                    palette::link_blue()
                                })
                                .into_element()
                            } else {
                                text(total.to_string())
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .tabular_nums()
                                    .text_color(color)
                                    .into_element()
                            }
                        })),
                )
        })
        .child({
            let run_history_test_id = format!("sidebar-run-history-{id}");
            button()
                .test_id(&run_history_test_id)
                .accessibility_label("Run history")
                .on_press(move || store::open_run_history_for_project(id))
                .rounded(px(6.5))
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .px(px(9.0))
                        .py(px(5.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(7.0))
                                .child(icons::clock_progress(12.5, palette::text_muted()))
                                .child(
                                    text("Run history")
                                        .text_size(px(12.0))
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            text(move || {
                                store::work_queue_for_project(id)
                                    .total_run_count()
                                    .to_string()
                            })
                            .text_size(px(11.0))
                            .tabular_nums()
                            .text_color(palette::text_muted()),
                        ),
                )
        })
        .child({
            let is_settings_active = move || {
                store::active_view() == ActiveView::ProjectSettings
                    && store::selected_project_id() == Some(id)
            };
            let settings_test_id = format!("sidebar-project-settings-{id}");
            button()
                .test_id(&settings_test_id)
                .accessibility_label(move || {
                    let name = store::project_by_id(id).map(|p| p.name).unwrap_or_default();
                    Some(SharedString::from(format!("Project settings for {name}")))
                })
                .on_press(move || store::open_project_settings_for_project(id))
                .rounded(px(6.5))
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(7.0))
                        .px(px(9.0))
                        .py(px(5.0))
                        .rounded(px(6.5))
                        .bg(move || {
                            if is_settings_active() {
                                palette::nav_active_bg()
                            } else {
                                AdaptiveColor::TRANSPARENT
                            }
                        })
                        .child(icons::settings_gear(12.5, move || {
                            if is_settings_active() {
                                palette::nav_active_fg()
                            } else {
                                palette::text_muted()
                            }
                        }))
                        .child(text("Project settings").text_size(px(12.0)).text_color(
                            move || {
                                if is_settings_active() {
                                    palette::nav_active_fg()
                                } else {
                                    palette::text_primary()
                                }
                            },
                        )),
                )
        })
}

// --- Content Router ---------------------------------------------------------------------

fn content_pane() -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .bg(palette::canvas_bg())
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .child(when(
                    || store::active_view() == ActiveView::Dashboard,
                    dashboard::dashboard_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Inbox,
                    inbox::inbox_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Machines,
                    machines::machines_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::WorkQueue,
                    work_queue::work_queue_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Workflows,
                    workflows::workflows_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::WorkflowEditor,
                    workflow_editor::workflow_editor_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::WorkflowRun,
                    workflow_run::workflow_run_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::ProjectSettings,
                    project_settings::project_settings_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Workspace,
                    workspace::workflow_workspace,
                ))
                .child(when(
                    || store::active_view() == ActiveView::RegisterProject,
                    register_project::register_project_view,
                ))
                .child(when(
                    || store::active_view() == ActiveView::Chat,
                    chat::chat_view,
                )),
        )
        .child(when(|| store::status_message().is_some(), status_banner))
}

fn workflow_run_footer_bar() -> impl IntoElement {
    view()
        .h(px(28.0))
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(14.0))
        .bg(palette::header_bg())
        .border_t_1()
        .border_color(palette::border_color())
        .child(dynamic(|| {
            let run = store::active_workflow_run();
            let dot_color = match run.execution_state {
                RunExecutionState::Running | RunExecutionState::Succeeded => {
                    palette::provider_dot_green()
                }
                RunExecutionState::Paused | RunExecutionState::NeedsApproval => {
                    palette::review_amber()
                }
                RunExecutionState::Failed | RunExecutionState::Stopped => {
                    palette::destructive_text()
                }
            };
            view()
                .flex_row()
                .items_center()
                .gap(px(14.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.5))
                        .child(view().w(px(6.5)).h(px(6.5)).rounded_full().bg(dot_color))
                        .child(
                            text(run.footer_summary())
                                .text_size(px(11.0))
                                .text_color(palette::text_secondary()),
                        ),
                )
                .child(
                    text(format!("Elapsed {}", run.total_elapsed))
                        .text_size(px(11.0))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(format!("Est. cost {}", run.budget_guard.formatted_spent()))
                        .text_size(px(11.0))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                )
        }))
        .child(
            button()
                .accessibility_label("Active extensions")
                .on_press(|| store::open_preferences_tab(PreferencesTab::Extensions))
                .hover(|s| s.opacity(0.8))
                .child(
                    text(move || {
                        let count = store::active_workflow_run().active_extensions_count;
                        format!("{count} extensions active")
                    })
                    .text_size(px(11.0))
                    .text_color(palette::text_muted()),
                ),
        )
}

fn status_banner() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(18.0))
        .py(px(7.0))
        .bg(palette::header_bg())
        .border_t_1()
        .border_color(palette::border_color())
        .child(
            text(move || store::status_message().unwrap_or_default())
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}
