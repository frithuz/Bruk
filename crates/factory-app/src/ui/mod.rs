//! Main macOS desktop shell: unified header toolbar, left navigation rail, and active canvas.

mod dashboard;
mod icons;
mod inbox;
mod machines;
mod palette;
pub(crate) mod preferences;
mod register_project;
mod work_queue;
mod workspace;

use factory_core::{DashboardGranularity, ProjectColor, ProjectId, WorkQueueViewMode};
use nsh_base::{SelectItem, SelectValue, button, select};
use nsh_core::prelude::*;

use crate::store::{self, ActiveView};

const ALL_PROJECTS_VALUE: &str = "__all_projects__";

/// Top-level macOS shell matching the reference Dashboard layout.
pub(crate) fn shell() -> impl IntoElement {
    view()
        .flex_1()
        .bg(palette::canvas_bg())
        .child(top_header_bar())
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .flex_row()
                .child(when(store::sidebar_visible, sidebar))
                .child(content_pane()),
        )
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
                    || store::active_view() == ActiveView::WorkQueue,
                    work_queue_header_controls,
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
                ActiveView::RegisterProject => "Register Project".to_owned(),
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
                    format!("{} · {}", snap.filter_label, snap.date_range_label)
                }
                ActiveView::Inbox => {
                    let pending = store::pending_inbox_count();
                    if pending == 0 {
                        "No pending agent requests".to_owned()
                    } else if pending == 1 {
                        "1 request waiting for your input".to_owned()
                    } else {
                        format!("{pending} requests waiting for your input")
                    }
                }
                ActiveView::Machines => {
                    let total = store::machines().len();
                    let disconnected = store::disconnected_machine_count();
                    let connected = total.saturating_sub(disconnected);
                    if disconnected == 0 {
                        format!("{connected} connected")
                    } else {
                        format!("{connected} connected · {disconnected} disconnected")
                    }
                }
                ActiveView::WorkQueue => {
                    let queue = store::active_work_queue();
                    format!(
                        "{} · {} · {}",
                        queue.project_name, queue.tracker_name, queue.repo_slug
                    )
                }
                ActiveView::RegisterProject => "Track local repositories in SQLite".to_owned(),
                ActiveView::Workspace => store::selected_project()
                    .map(|p| format!("{} · {}", p.default_branch, p.root_path.display()))
                    .unwrap_or_else(|| "Select a project to run workflows".to_owned()),
            })
            .text_size(px(11.5))
            .text_color(palette::text_muted()),
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
                .child(icons::sync_arrows(palette::text_secondary()))
                .child(
                    text("Sync")
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
                            text("No projects tracked yet")
                                .px(px(8.0))
                                .text_size(px(12.0))
                                .text_color(palette::text_muted())
                        },
                    ))
                    .child(each(
                        store::project_ids,
                        |id: &ProjectId| *id,
                        |id, _| sidebar_project_row(id.get_untracked()),
                    )),
            ),
        )
        .child(
            view()
                .pt(px(8.0))
                .gap(px(8.0))
                .border_t_1()
                .border_color(palette::divider_color())
                .child(sidebar_agent_slots_summary())
                .child(sidebar_preferences_item()),
        )
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

fn sidebar_preferences_item() -> impl IntoElement {
    button()
        .accessibility_label("Preferences")
        .on_press(store::open_preferences)
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
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(9.0))
                        .child(icons::settings_gear(15.0, palette::text_secondary()))
                        .child(
                            text("Preferences")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    text("⌘,")
                        .text_size(px(11.0))
                        .text_color(palette::text_muted()),
                ),
        )
}

fn sidebar_dashboard_item() -> impl IntoElement {
    let is_active = || store::active_view() == ActiveView::Dashboard;
    button()
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
                .child(when(
                    || store::disconnected_machine_count() > 0,
                    move || {
                        text(move || {
                            format!("{}\ndisconnected", store::disconnected_machine_count())
                        })
                        .text_size(px(10.5))
                        .text_color(move || {
                            if is_active() {
                                palette::nav_active_fg()
                            } else {
                                palette::warning_text()
                            }
                        })
                    },
                )),
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
                                store::work_queue_for_project(id)
                                    .active_queue_count()
                                    .to_string()
                            })
                            .text_size(px(11.5))
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
        .child(
            text("Workflows")
                .px(px(9.0))
                .pt(px(5.0))
                .pb(px(1.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        )
        .child(dynamic(move || {
            let queue = store::work_queue_for_project(id);
            let mut list = view().gap(px(1.5));
            for wf in &queue.workflows {
                let wf_name = wf.name.clone();
                let wf_click = wf_name.clone();
                let running = queue.running_count_for_workflow(&wf.name);
                let trailing = if running > 0 {
                    Some(format!("{running} running"))
                } else if wf.ad_hoc_only {
                    Some("Ad-hoc".to_owned())
                } else {
                    None
                };

                list = list.child(
                    button()
                        .on_press(move || store::open_project_workflow(id, &wf_click))
                        .rounded(px(6.0))
                        .hover(|s| s.bg(palette::sidebar_hover_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(6.0))
                                .px(px(9.0))
                                .py(px(4.5))
                                .child(
                                    view()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .flex_row()
                                        .items_center()
                                        .gap(px(7.0))
                                        .child(icons::workflow_branch(12.5, palette::text_muted()))
                                        .child(
                                            text(wf_name)
                                                .flex_1()
                                                .text_size(px(12.0))
                                                .text_color(palette::text_primary()),
                                        ),
                                )
                                .child(if let Some(meta) = trailing {
                                    text(meta)
                                        .text_size(px(10.5))
                                        .tabular_nums()
                                        .text_color(palette::text_muted())
                                        .into_element()
                                } else {
                                    view().into_element()
                                }),
                        ),
                );
            }
            list
        }))
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
                    || store::active_view() == ActiveView::Workspace,
                    workspace::workflow_workspace,
                ))
                .child(when(
                    || store::active_view() == ActiveView::RegisterProject,
                    register_project::register_project_view,
                )),
        )
        .child(when(|| store::status_message().is_some(), status_banner))
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
