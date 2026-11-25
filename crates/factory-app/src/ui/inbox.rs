//! Inbox view inside the primary window (`⌘I`, sidebar, and menu bar entryway)
//! matching `08 Inbox.png`.
//!
//! Displays every blocking agent request (permissions, questions, review gates,
//! and exhausted loops) in an oldest-first master-detail layout with keyboard
//! shortcuts, inline command editing, Always Allow rule previews, and answered
//! history.

use factory_core::{
    AgentBadge, AlwaysAllowScope, AnsweredInboxItem, AttentionRequest, AttentionRequestBody,
    AttentionRequestKind, InboxFilter, LoopExhaustedDetail, PermissionDetail, QuestionDetail,
    ReviewDetail,
};
use nsh_base::{button, input};
use nsh_core::{
    element::{KeyOptions, on_key_with},
    keymap::When,
    prelude::*,
};

use crate::{
    store,
    ui::{icons, palette},
};

/// Right-hand toolbar controls rendered in the main window header while viewing Inbox.
pub(crate) fn inbox_header_controls() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .window_drag(false)
        .child(inbox_filter_segmented_control())
        .child(pause_all_agents_button())
}

/// Primary-window Inbox view: oldest-first master-detail split plus shortcut footer bar.
pub(crate) fn inbox_view() -> impl IntoElement {
    bind_inbox_shortcuts();

    view()
        .flex_1()
        .min_h(px(0.0))
        .bg(palette::canvas_bg())
        .text_color(palette::text_primary())
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .flex_row()
                .child(inbox_master_list())
                .child(inbox_detail_pane()),
        )
        .child(inbox_shortcut_footer())
}

fn bind_inbox_shortcuts() {
    let auto = KeyOptions {
        when: When::Auto,
        ..KeyOptions::default()
    };
    let always = KeyOptions {
        when: When::Always,
        ..KeyOptions::default()
    };
    let not_editing = KeyOptions {
        when: When::NotEditing,
        ..KeyOptions::default()
    };

    on_key_with("up", auto, |_| store::inbox_select_relative(-1));
    on_key_with("down", auto, |_| store::inbox_select_relative(1));
    on_key_with("mod+enter", always, |_| store::inbox_primary_answer());
    on_key_with("alt+mod+enter", always, |_| store::inbox_allow_for_run());
    on_key_with("mod+backspace", auto, |_| {
        store::inbox_secondary_deny_shortcut()
    });
    on_key_with("e", not_editing, |_| store::toggle_inbox_editing_command());
    on_key_with("o", not_editing, |_| store::inbox_open_selected_run());
}

// --- Toolbar Controls -------------------------------------------------------------------

fn inbox_filter_segmented_control() -> impl IntoElement {
    let mut track = view()
        .flex_row()
        .items_center()
        .h(px(30.0))
        .p(px(2.5))
        .rounded(px(8.0))
        .bg(palette::segmented_track_bg())
        .gap(px(2.0));

    for filter in InboxFilter::ALL {
        track = track.child(inbox_filter_segment(*filter));
    }

    track
}

fn inbox_filter_segment(filter: InboxFilter) -> impl IntoElement {
    let is_active = move || store::inbox_filter() == filter;
    let test_id = match filter {
        InboxFilter::All => "inbox-filter-all",
        InboxFilter::Permissions => "inbox-filter-permissions",
        InboxFilter::Questions => "inbox-filter-questions",
        InboxFilter::Reviews => "inbox-filter-reviews",
        InboxFilter::Setup => "inbox-filter-setup",
    };

    button()
        .test_id(test_id)
        .accessibility_label(filter.label())
        .on_press(move || store::set_inbox_filter(filter))
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
            text(move || match filter {
                InboxFilter::All => format!("All {}", store::pending_inbox_count()),
                other => other.label().to_owned(),
            })
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .tabular_nums()
            .text_color(move || {
                if is_active() {
                    palette::text_primary()
                } else {
                    palette::text_muted()
                }
            }),
        )
}

fn pause_all_agents_button() -> impl IntoElement {
    button()
        .test_id("inbox-pause-all")
        .accessibility_label(move || {
            Some(SharedString::from(
                if store::inbox_state().all_agents_paused {
                    "Resume All Agents"
                } else {
                    "Pause All Agents"
                },
            ))
        })
        .on_press(store::toggle_pause_all_agents)
        .h(px(30.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(move || {
            if store::inbox_state().all_agents_paused {
                palette::pill_warning_border()
            } else {
                palette::border_color()
            }
        })
        .bg(move || {
            if store::inbox_state().all_agents_paused {
                palette::pill_warning_bg()
            } else {
                palette::control_bg()
            }
        })
        .flex_row()
        .items_center()
        .gap(px(6.5))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(dynamic(|| {
            if store::inbox_state().all_agents_paused {
                icons::play_filled(palette::pill_warning_text()).into_element()
            } else {
                icons::pause(palette::text_primary()).into_element()
            }
        }))
        .child(
            text(move || {
                if store::inbox_state().all_agents_paused {
                    "Resume All Agents".to_owned()
                } else {
                    "Pause All Agents".to_owned()
                }
            })
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || {
                if store::inbox_state().all_agents_paused {
                    palette::pill_warning_text()
                } else {
                    palette::text_primary()
                }
            }),
        )
}

// --- Left Master List -------------------------------------------------------------------

fn inbox_master_list() -> impl IntoElement {
    view()
        .w(px(326.0))
        .flex_shrink_0()
        .bg(palette::sidebar_bg())
        .border_r_1()
        .border_color(palette::border_color())
        .child(scroll().flex_1().min_h(px(0.0)).child(dynamic(|| {
            let inbox = store::inbox_state();
            let filter = store::inbox_filter();
            let selected_id = store::inbox_selected_id();
            let answered_expanded = store::inbox_answered_expanded();
            let pending: Vec<AttentionRequest> = inbox
                .pending_oldest_first(filter)
                .into_iter()
                .cloned()
                .collect();

            let mut col = view().p(px(10.0)).gap(px(6.0));

            col = col.child(
                text("Blocking a run · oldest first")
                    .px(px(6.0))
                    .pt(px(2.0))
                    .pb(px(4.0))
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_muted()),
            );

            if pending.is_empty() {
                col = col.child(
                    view()
                        .mx(px(4.0))
                        .p(px(14.0))
                        .rounded(px(9.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .gap(px(3.0))
                        .child(
                            text(if inbox.pending_count() == 0 {
                                "No waiting requests"
                            } else {
                                "No matching requests"
                            })
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                        )
                        .child(
                            text(if inbox.pending_count() == 0 {
                                if inbox.active_agents_count == 0 {
                                    "Permissions, questions, and review gates will appear here when workflows need your input."
                                } else {
                                    "Agents are running autonomously."
                                }
                            } else {
                                "Switch to All to see other waiting agents."
                            })
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                        ),
                );
            } else {
                for req in &pending {
                    let is_selected = selected_id.as_deref() == Some(req.id.as_str());
                    col = col.child(pending_list_row(req, is_selected));
                }
            }

            // Collapsible "Answered today · N" disclosure section.
            let answered_count = inbox.answered_today.len();
            if answered_count > 0 {
                col = col.child(
                    view().pt(px(8.0)).gap(px(4.0)).child(
                        button()
                            .test_id("inbox-answered-toggle")
                            .accessibility_label("Toggle Answered today")
                            .on_press(store::toggle_inbox_answered_expanded)
                            .px(px(6.0))
                            .py(px(5.0))
                            .rounded(px(6.0))
                            .flex_row()
                            .items_center()
                            .gap(px(5.0))
                            .hover(|s| s.bg(palette::sidebar_hover_bg()))
                            .child(if answered_expanded {
                                icons::chevron_down(palette::text_muted()).into_element()
                            } else {
                                icons::chevron_right(palette::text_muted()).into_element()
                            })
                            .child(
                                text(format!("Answered today · {answered_count}"))
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .tabular_nums()
                                    .text_color(palette::text_muted()),
                            ),
                    ),
                );

                if answered_expanded {
                    for ans in &inbox.answered_today {
                        let is_selected = selected_id.as_deref() == Some(ans.id.as_str());
                        col = col.child(answered_list_row(ans, is_selected));
                    }
                }
            }

            col
        })))
}

fn kind_icon(kind: AttentionRequestKind, selected: bool) -> AnyElement {
    let color = if selected {
        palette::nav_active_fg()
    } else {
        match kind {
            AttentionRequestKind::LoopExhausted => palette::workflow_now_amber(),
            AttentionRequestKind::Review => palette::review_amber(),
            AttentionRequestKind::Question => palette::link_blue(),
            AttentionRequestKind::Permission => palette::text_secondary(),
            AttentionRequestKind::Setup => palette::warning_icon(),
        }
    };
    match kind {
        AttentionRequestKind::LoopExhausted => icons::loop_arrows(color).into_element(),
        AttentionRequestKind::Review => icons::shield_approval(color).into_element(),
        AttentionRequestKind::Question => icons::question_bubble(color).into_element(),
        AttentionRequestKind::Permission => icons::lock_permission(color).into_element(),
        AttentionRequestKind::Setup => icons::alert_circle(color).into_element(),
    }
}

fn pending_list_row(req: &AttentionRequest, selected: bool) -> impl IntoElement {
    let id = req.id.clone();
    let test_id = format!("inbox-row-{}", req.id);
    let title_color = if selected {
        palette::nav_active_fg()
    } else {
        palette::text_primary()
    };
    let meta_color = if selected {
        palette::inbox_selected_muted_fg()
    } else {
        palette::text_muted()
    };
    let subtitle_color = if selected {
        palette::nav_active_fg()
    } else {
        palette::text_secondary()
    };

    let mut content = view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(2.5))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    text(req.inbox_title.clone())
                        .flex_1()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(title_color)
                        .truncate(),
                )
                .child(
                    text(req.compact_age())
                        .text_size(px(11.5))
                        .tabular_nums()
                        .text_color(meta_color),
                ),
        )
        .child(
            text(req.breadcrumb())
                .text_size(px(11.5))
                .text_color(meta_color)
                .truncate(),
        )
        .child(
            text(req.inbox_subtitle.clone())
                .text_size(px(12.0))
                .text_color(subtitle_color)
                .truncate(),
        );

    if let Some(deadline) = &req.deadline_badge {
        content = content.child(
            view().pt(px(3.0)).flex_row().child(
                view()
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded_full()
                    .border_1()
                    .border_color(if selected {
                        palette::inbox_selected_pill_border()
                    } else {
                        palette::divider_color()
                    })
                    .bg(if selected {
                        palette::inbox_selected_pill_bg()
                    } else {
                        palette::subtle_surface_bg()
                    })
                    .child(
                        text(deadline.clone())
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .tabular_nums()
                            .text_color(if selected {
                                palette::nav_active_fg()
                            } else {
                                palette::text_muted()
                            }),
                    ),
            ),
        );
    }

    button()
        .test_id(&test_id)
        .accessibility_label(format!("Select {}", req.inbox_title))
        .on_press(move || store::inbox_select_request(&id))
        .w_full()
        .px(px(10.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .bg(if selected {
            palette::nav_active_bg()
        } else {
            AdaptiveColor::TRANSPARENT
        })
        .hover(move |s| {
            if selected {
                s
            } else {
                s.bg(palette::sidebar_hover_bg())
            }
        })
        .active(|s| s.opacity(0.9))
        .flex_row()
        .items_start()
        .gap(px(9.0))
        .child(view().pt(px(2.0)).child(kind_icon(req.kind, selected)))
        .child(content)
}

fn answered_list_row(item: &AnsweredInboxItem, selected: bool) -> impl IntoElement {
    let id = item.id.clone();
    let test_id = format!("inbox-answered-row-{}", item.id);
    let title_color = if selected {
        palette::nav_active_fg()
    } else {
        palette::text_secondary()
    };
    let meta_color = if selected {
        palette::inbox_selected_muted_fg()
    } else {
        palette::text_muted()
    };

    button()
        .test_id(&test_id)
        .accessibility_label(format!("View answered request {}", item.title))
        .on_press(move || store::inbox_select_request(&id))
        .w_full()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(9.0))
        .bg(if selected {
            palette::nav_active_bg()
        } else {
            AdaptiveColor::TRANSPARENT
        })
        .hover(move |s| {
            if selected {
                s
            } else {
                s.bg(palette::sidebar_hover_bg())
            }
        })
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .child(view().pt(px(2.0)).child(icons::check(if selected {
            palette::nav_active_fg()
        } else {
            palette::success_text()
        })))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.0))
                        .child(
                            text(item.subtitle.clone())
                                .flex_1()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(title_color)
                                .truncate(),
                        )
                        .child(
                            text(item.answered_ago.clone())
                                .text_size(px(10.5))
                                .text_color(meta_color),
                        ),
                )
                .child(
                    text(item.outcome.clone())
                        .text_size(px(11.0))
                        .text_color(meta_color)
                        .truncate(),
                ),
        )
}

// --- Right Detail Pane ------------------------------------------------------------------

fn inbox_detail_pane() -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .bg(palette::card_bg())
        .child(dynamic(|| {
            if let Some(req) = store::inbox_selected_pending() {
                pending_detail_surface(&req).into_element()
            } else if let Some(ans) = store::inbox_selected_answered() {
                answered_detail_surface(&ans).into_element()
            } else {
                empty_detail_surface().into_element()
            }
        }))
}

fn empty_detail_surface() -> impl IntoElement {
    let active_agents = store::inbox_state().active_agents_count;
    let subtitle = if active_agents == 0 {
        "No agents are waiting for your input right now. Blocking permission requests, questions, and review gates will appear here when workflows run."
    } else {
        "No agents are waiting for your input right now. Other runs keep going in the background."
    };

    view()
        .test_id("inbox-empty-state")
        .flex_1()
        .items_center()
        .justify_center()
        .p(px(32.0))
        .gap(px(8.0))
        .child(
            view()
                .w(px(40.0))
                .h(px(40.0))
                .rounded_full()
                .bg(palette::status_success_bg())
                .items_center()
                .justify_center()
                .child(icons::check(palette::success_text())),
        )
        .child(
            text("All caught up")
                .text_size(px(18.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            text(subtitle)
                .text_center()
                .text_size(px(13.0))
                .text_color(palette::text_muted()),
        )
}

fn answered_detail_surface(item: &AnsweredInboxItem) -> impl IntoElement {
    let mut card = view()
        .p(px(16.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    view()
                        .px(px(9.0))
                        .py(px(3.0))
                        .rounded_full()
                        .bg(palette::status_success_bg())
                        .child(
                            text(item.outcome.clone())
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::success_text()),
                        ),
                )
                .child(
                    text(item.answered_ago.clone())
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            text(item.subtitle.clone())
                .app_class("font-mono")
                .text_size(px(12.5))
                .text_color(palette::text_primary()),
        );

    if let Some(note) = &item.note {
        card = card.child(
            text(format!("Note sent: {note}"))
                .text_size(px(12.5))
                .text_color(palette::text_secondary()),
        );
    }

    scroll().flex_1().min_h(px(0.0)).child(
        view()
            .p(px(26.0))
            .gap(px(14.0))
            .child(
                text(item.breadcrumb())
                    .text_size(px(12.0))
                    .text_color(palette::text_muted()),
            )
            .child(
                text(item.title.clone())
                    .text_size(px(21.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_primary()),
            )
            .child(
                view().flex_row().items_center().gap(px(10.0)).child(
                    button()
                        .test_id("inbox-open-run")
                        .on_press(store::inbox_open_selected_run)
                        .child(
                            text("Open run")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::link_blue()),
                        ),
                ),
            )
            .child(card),
    )
}

fn pending_detail_surface(req: &AttentionRequest) -> impl IntoElement {
    let feedback = store::inbox_feedback();
    let waiting_rules_open = store::inbox_waiting_rules_open();

    let mut body = view().p(px(26.0)).gap(px(16.0)).child(detail_header(req));

    if let Some(msg) = feedback {
        body = body.child(
            view()
                .px(px(12.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::intake_banner_border())
                .bg(palette::intake_banner_bg())
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(icons::info_circle(palette::link_blue()))
                .child(
                    text(msg)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        );
    }

    body = body.child(agent_why_callout(req.agent, &req.why_text));

    match &req.body {
        AttentionRequestBody::Permission(detail) => {
            body = body.child(permission_detail_section(req, detail));
        }
        AttentionRequestBody::Question(detail) => {
            body = body.child(question_detail_section(req, detail));
        }
        AttentionRequestBody::Review(detail) => {
            body = body.child(review_detail_section(req, detail));
        }
        AttentionRequestBody::LoopExhausted(detail) => {
            body = body.child(loop_exhausted_detail_section(req, detail));
        }
        AttentionRequestBody::McpProblem(detail) => {
            body = body.child(mcp_problem_detail_section(req, detail));
        }
        AttentionRequestBody::SecretProblem(detail) => {
            body = body.child(secret_problem_detail_section(req, detail));
        }
    }

    let mut pane = view()
        .flex_1()
        .min_h(px(0.0))
        .child(scroll().flex_1().min_h(px(0.0)).child(body));

    if waiting_rules_open {
        pane = pane.child(waiting_rules_panel());
    }

    pane.child(detail_timeout_bar(&req.timeout_footer))
}

fn detail_header(req: &AttentionRequest) -> impl IntoElement {
    view()
        .gap(px(8.0))
        .child(
            text(req.breadcrumb())
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
        .child(
            text(req.inbox_title.clone())
                .text_size(px(22.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    view()
                        .px(px(9.0))
                        .py(px(3.5))
                        .rounded_full()
                        .bg(palette::status_warning_bg())
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .child(icons::pause(palette::attempt_banner_text()))
                        .child(
                            text(req.status_pill.clone())
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::attempt_banner_text()),
                        ),
                )
                .child(
                    view()
                        .px(px(9.5))
                        .py(px(3.5))
                        .rounded_full()
                        .bg(palette::subtle_surface_bg())
                        .child(
                            text("Other runs keep going")
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    button()
                        .test_id("inbox-open-run")
                        .accessibility_label("Open run")
                        .on_press(store::inbox_open_selected_run)
                        .px(px(4.0))
                        .py(px(2.0))
                        .active(|s| s.opacity(0.75))
                        .child(
                            text("Open run")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::link_blue()),
                        ),
                ),
        )
}

fn agent_why_callout(agent: AgentBadge, why_text: &str) -> impl IntoElement {
    view()
        .px(px(14.0))
        .py(px(11.0))
        .rounded(px(9.0))
        .bg(palette::subtle_surface_bg())
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .child(
            view()
                .w(px(22.0))
                .h(px(22.0))
                .flex_shrink_0()
                .rounded(px(5.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_bg())
                .items_center()
                .justify_center()
                .child(
                    text(agent.short_code())
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .flex_wrap()
                .gap(px(4.0))
                .child(
                    text("Why:")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(why_text.to_owned())
                        .text_size(px(12.5))
                        .text_color(palette::text_secondary()),
                ),
        )
}

// --- Permission Detail ------------------------------------------------------------------

fn permission_detail_section(
    req: &AttentionRequest,
    detail: &PermissionDetail,
) -> impl IntoElement {
    let editing = store::inbox_editing_command();
    let edited_command = store::inbox_edited_command();
    let always_allow_open = store::inbox_always_allow_open();
    let always_allow_pattern = store::inbox_always_allow_pattern();
    let note_draft = store::inbox_note_draft();

    let mut risk_row = view().flex_row().flex_wrap().items_center().gap(px(7.0));
    for tag in &detail.risk_tags {
        let mut pill = view()
            .px(px(9.0))
            .py(px(3.5))
            .rounded_full()
            .border_1()
            .flex_row()
            .items_center()
            .gap(px(5.0));

        if tag.is_warning {
            pill = pill
                .border_color(palette::pill_warning_border())
                .bg(palette::pill_warning_bg());
        } else {
            pill = pill
                .border_color(palette::border_color())
                .bg(palette::card_bg());
        }

        if tag.has_globe {
            pill = pill.child(icons::globe(if tag.is_warning {
                palette::pill_warning_text()
            } else {
                palette::text_muted()
            }));
        }

        pill = pill.child(
            text(tag.label.clone())
                .text_size(px(11.5))
                .font_weight(if tag.is_warning {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if tag.is_warning {
                    palette::pill_warning_text()
                } else {
                    palette::text_secondary()
                }),
        );
        risk_row = risk_row.child(pill);
    }

    let command_card = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .px(px(14.0))
                .py(px(9.0))
                .bg(palette::subtle_surface_bg())
                .border_b_1()
                .border_color(palette::border_color())
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            text("Run command")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(format!(
                                "on {} in {}",
                                detail.machine_name, detail.working_dir
                            ))
                            .text_size(px(12.0))
                            .text_color(palette::text_muted())
                            .truncate(),
                        ),
                )
                .child(
                    button()
                        .test_id("inbox-edit-command-btn")
                        .accessibility_label("Edit Before Allowing")
                        .on_press(store::toggle_inbox_editing_command)
                        .h(px(26.0))
                        .px(px(10.0))
                        .rounded(px(6.5))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::sidebar_hover_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text(if editing {
                                "Done Editing"
                            } else {
                                "Edit Before Allowing"
                            })
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_primary()),
                        ),
                ),
        )
        .child(
            view()
                .p(px(14.0))
                .gap(px(12.0))
                .child(if editing {
                    input()
                        .test_id("inbox-command-input")
                        .value(edited_command)
                        .on_change(|value: &str| store::set_inbox_edited_command(value.to_owned()))
                        .h(px(34.0))
                        .px(px(10.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::nav_active_bg())
                        .bg(palette::control_bg())
                        .app_class("font-mono")
                        .text_size(px(13.0))
                        .text_color(palette::text_primary())
                        .into_element()
                } else {
                    text(if edited_command.trim().is_empty() {
                        detail.command.clone()
                    } else {
                        edited_command
                    })
                    .app_class("font-mono")
                    .text_size(px(13.0))
                    .text_color(palette::text_primary())
                    .into_element()
                })
                .child(risk_row),
        );

    let actions_row = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    button()
                        .test_id("inbox-allow-once")
                        .accessibility_label("Allow Once")
                        .on_press(store::inbox_allow_once)
                        .h(px(34.0))
                        .px(px(16.0))
                        .rounded(px(8.0))
                        .bg(palette::nav_active_bg())
                        .items_center()
                        .justify_center()
                        .active(|s| s.opacity(0.85))
                        .child(
                            text("Allow Once")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                )
                .child(
                    button()
                        .test_id("inbox-allow-run")
                        .accessibility_label("Allow for This Run")
                        .on_press(store::inbox_allow_for_run)
                        .h(px(34.0))
                        .px(px(14.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.82))
                        .child(
                            text("Allow for This Run")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    button()
                        .test_id("inbox-always-allow-btn")
                        .accessibility_label("Always Allow...")
                        .on_press(store::toggle_inbox_always_allow_open)
                        .h(px(34.0))
                        .px(px(14.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(if always_allow_open {
                            palette::nav_active_bg()
                        } else {
                            palette::border_color()
                        })
                        .bg(palette::control_bg())
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.82))
                        .child(
                            text("Always Allow...")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        )
                        .child(icons::chevron_down(palette::text_muted())),
                ),
        )
        .child(
            button()
                .test_id("inbox-deny-btn")
                .accessibility_label("Deny")
                .on_press(store::inbox_deny_selected)
                .h(px(34.0))
                .px(px(16.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::pill_error_bg()))
                .active(|s| s.opacity(0.82))
                .child(
                    text("Deny")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::destructive_text()),
                ),
        );

    let mut rule_box = view()
        .p(px(14.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::inbox_rule_box_border())
        .bg(palette::inbox_rule_box_bg())
        .gap(px(6.0))
        .child(
            text(format!(
                "Always Allow adds this rule to the {} workflow",
                detail.always_allow_workflow
            ))
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_primary()),
        );

    if always_allow_open {
        rule_box = rule_box
            .child(
                input()
                    .test_id("inbox-rule-pattern-input")
                    .value(always_allow_pattern)
                    .on_change(|v: &str| store::set_inbox_always_allow_pattern(v.to_owned()))
                    .h(px(32.0))
                    .px(px(10.0))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::control_bg())
                    .app_class("font-mono")
                    .text_size(px(12.5))
                    .text_color(palette::text_primary()),
            )
            .child(
                view()
                    .pt(px(2.0))
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(always_allow_scope_button(AlwaysAllowScope::Workflow))
                    .child(always_allow_scope_button(AlwaysAllowScope::Project))
                    .child(always_allow_scope_button(AlwaysAllowScope::Global)),
            );
    } else {
        rule_box = rule_box.child(
            text(detail.always_allow_pattern.clone())
                .app_class("font-mono")
                .text_size(px(12.5))
                .text_color(palette::text_primary()),
        );
    }

    rule_box = rule_box.child(
        text(detail.always_allow_note.clone())
            .text_size(px(11.5))
            .text_color(palette::text_muted()),
    );

    view()
        .gap(px(16.0))
        .child(command_card)
        .child(actions_row)
        .child(rule_box)
        .child(note_to_agent_section(
            "(sent with Deny, optional)",
            &req.note_placeholder,
            note_draft,
        ))
}

fn always_allow_scope_button(scope: AlwaysAllowScope) -> impl IntoElement {
    let test_id = match scope {
        AlwaysAllowScope::Workflow => "inbox-save-rule-workflow",
        AlwaysAllowScope::Project => "inbox-save-rule-project",
        AlwaysAllowScope::Global => "inbox-save-rule-global",
    };
    let is_primary = scope == AlwaysAllowScope::Workflow;
    button()
        .test_id(test_id)
        .accessibility_label(format!("Save Always Allow rule for {}", scope.label()))
        .on_press(move || store::inbox_always_allow(scope))
        .h(px(28.0))
        .px(px(11.0))
        .rounded(px(6.5))
        .border_1()
        .border_color(if is_primary {
            palette::nav_active_bg()
        } else {
            palette::border_color()
        })
        .bg(if is_primary {
            palette::nav_active_bg()
        } else {
            palette::control_bg()
        })
        .items_center()
        .justify_center()
        .active(|s| s.opacity(0.82))
        .child(
            text(format!("Save · {}", scope.label()))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if is_primary {
                    palette::nav_active_fg()
                } else {
                    palette::text_primary()
                }),
        )
}

// --- Question Detail --------------------------------------------------------------------

fn question_detail_section(req: &AttentionRequest, detail: &QuestionDetail) -> impl IntoElement {
    let chosen = store::inbox_question_choice();
    let note_draft = store::inbox_note_draft();

    let mut options_col = view().gap(px(8.0));
    for opt in &detail.options {
        let is_selected = chosen.as_deref() == Some(opt.label.as_str());
        let label = opt.label.clone();
        let test_id = format!("inbox-question-opt-{}", opt.id);

        options_col = options_col.child(
            button()
                .test_id(&test_id)
                .accessibility_label(opt.label.clone())
                .on_press(move || store::set_inbox_question_choice(label.clone()))
                .w_full()
                .p(px(12.0))
                .rounded(px(9.0))
                .border_1()
                .border_color(if is_selected {
                    palette::card_selected_border()
                } else {
                    palette::border_color()
                })
                .bg(if is_selected {
                    palette::card_selected_bg()
                } else {
                    palette::control_bg()
                })
                .items_start()
                .gap(px(3.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(opt.label.clone())
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(when(opt.recommended, || {
                            view()
                                .px(px(7.0))
                                .py(px(1.5))
                                .rounded_full()
                                .bg(palette::status_running_bg())
                                .child(
                                    text("Recommended")
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::link_blue()),
                                )
                        })),
                )
                .child(
                    text(opt.detail.clone())
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        );
    }

    let question_card = view()
        .p(px(16.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(12.0))
        .child(
            text(detail.prompt.clone())
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(options_col);

    let actions = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    button()
                        .test_id("inbox-send-answer")
                        .accessibility_label("Send Answer")
                        .on_press(store::inbox_answer_question)
                        .h(px(34.0))
                        .px(px(16.0))
                        .rounded(px(8.0))
                        .bg(palette::nav_active_bg())
                        .items_center()
                        .justify_center()
                        .active(|s| s.opacity(0.85))
                        .child(
                            text("Send Answer")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                )
                .child(
                    button()
                        .test_id("inbox-let-agent-decide")
                        .accessibility_label("Let Agent Decide")
                        .on_press(store::inbox_let_agent_decide)
                        .h(px(34.0))
                        .px(px(14.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Let Agent Decide")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
        .child(
            text(format!("Default: {}", detail.default_if_unanswered))
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        );

    view()
        .gap(px(16.0))
        .child(question_card)
        .child(actions)
        .child(note_to_agent_section(
            "(custom answer, optional)",
            &req.note_placeholder,
            note_draft,
        ))
}

// --- Review Gate Detail -----------------------------------------------------------------

fn review_detail_section(req: &AttentionRequest, detail: &ReviewDetail) -> impl IntoElement {
    let note_draft = store::inbox_note_draft();

    let mut files_list = view().gap(px(6.0));
    for file in &detail.files {
        files_list = files_list.child(
            view()
                .px(px(12.0))
                .py(px(8.0))
                .rounded(px(7.0))
                .bg(palette::subtle_surface_bg())
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(file.path.clone())
                        .app_class("font-mono")
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(format!("+{}", file.additions))
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .tabular_nums()
                                .text_color(palette::diff_add_text()),
                        )
                        .child(
                            text(format!("-{}", file.deletions))
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .tabular_nums()
                                .text_color(palette::diff_del_text()),
                        ),
                ),
        );
    }

    let summary_card = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .px(px(14.0))
                .py(px(9.0))
                .bg(palette::subtle_surface_bg())
                .border_b_1()
                .border_color(palette::border_color())
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(icons::git_branch(palette::text_secondary()))
                        .child(
                            text(format!("{} → {}", detail.branch, detail.base_branch))
                                .app_class("font-mono")
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    text(detail.tests_summary.clone())
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::success_text()),
                ),
        )
        .child(
            view()
                .p(px(14.0))
                .gap(px(12.0))
                .child(
                    text(detail.summary.clone())
                        .text_size(px(13.0))
                        .text_color(palette::text_secondary()),
                )
                .child(files_list),
        );

    let actions = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    button()
                        .test_id("inbox-approve-review")
                        .accessibility_label("Approve & Open PR")
                        .on_press(store::inbox_approve_review)
                        .h(px(34.0))
                        .px(px(16.0))
                        .rounded(px(8.0))
                        .bg(palette::nav_active_bg())
                        .items_center()
                        .justify_center()
                        .active(|s| s.opacity(0.85))
                        .child(
                            text("Approve & Open PR")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                )
                .child(
                    button()
                        .test_id("inbox-request-changes")
                        .accessibility_label("Request Changes")
                        .on_press(store::inbox_request_changes)
                        .h(px(34.0))
                        .px(px(14.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Request Changes")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
        .child(
            button()
                .test_id("inbox-stop-review-run")
                .accessibility_label("Stop Run")
                .on_press(store::inbox_stop_exhausted_run)
                .h(px(34.0))
                .px(px(14.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::pill_error_bg()))
                .child(
                    text("Stop Run")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::destructive_text()),
                ),
        );

    view()
        .gap(px(16.0))
        .child(summary_card)
        .child(actions)
        .child(note_to_agent_section(
            "(sent with Request Changes, optional)",
            &req.note_placeholder,
            note_draft,
        ))
}

// --- Loop Exhausted Detail --------------------------------------------------------------

fn mcp_problem_detail_section(
    req: &AttentionRequest,
    detail: &factory_core::inbox::McpProblemDetail,
) -> impl IntoElement {
    let mut actions = view().flex_row().items_center().gap(px(10.0));
    if let Some(label) = detail.problem.fix.button_label() {
        actions = actions.child(
            button()
                .test_id("inbox-mcp-fix")
                .accessibility_label(label)
                .on_press(store::inbox_fix_setup_item)
                .h(px(34.0))
                .px(px(16.0))
                .rounded(px(8.0))
                .bg(palette::nav_active_bg())
                .items_center()
                .justify_center()
                .active(|s| s.opacity(0.85))
                .child(
                    text(label)
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        );
    }
    actions = actions.child(
        button()
            .test_id("inbox-mcp-dismiss")
            .accessibility_label("Dismiss")
            .on_press(store::inbox_dismiss_setup_item)
            .h(px(34.0))
            .px(px(16.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::control_bg())
            .items_center()
            .justify_center()
            .hover(|s| s.bg(palette::subtle_surface_bg()))
            .child(
                text("Dismiss")
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_primary()),
            ),
    );
    view()
        .gap(px(14.0))
        .child(crate::ui::mcp::mcp_problem_banner(
            detail.problem.clone(),
            Some(req.project_name.clone()),
        ))
        .child(
            text(if detail.stopped_work {
                "Project MCP servers should work for every developer, so the work that needed this one stopped. It clears from the Inbox once the server starts."
            } else {
                "The agent ran without this server. It clears from the Inbox once the server starts."
            })
            .text_size(px(12.0))
            .text_color(palette::text_muted()),
        )
        .child(actions)
}

fn secret_problem_detail_section(
    req: &AttentionRequest,
    detail: &factory_core::SecretProblemDetail,
) -> impl IntoElement {
    let mut actions = view().flex_row().items_center().gap(px(10.0));
    if let Some(label) = detail.problem.fix.button_label() {
        actions = actions.child(
            button()
                .test_id("inbox-secret-fix")
                .accessibility_label(label)
                .on_press(store::inbox_fix_setup_item)
                .h(px(34.0))
                .px(px(16.0))
                .rounded(px(8.0))
                .bg(palette::nav_active_bg())
                .items_center()
                .justify_center()
                .active(|s| s.opacity(0.85))
                .child(
                    text(label)
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        );
    }
    actions = actions.child(
        button()
            .test_id("inbox-secret-dismiss")
            .accessibility_label("Dismiss")
            .on_press(store::inbox_dismiss_setup_item)
            .h(px(34.0))
            .px(px(16.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::control_bg())
            .items_center()
            .justify_center()
            .hover(|s| s.bg(palette::subtle_surface_bg()))
            .child(
                text("Dismiss")
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_primary()),
            ),
    );
    view()
        .gap(px(14.0))
        .child(crate::ui::secrets::secret_problem_banner(
            detail.problem.clone(),
            Some(req.project_name.clone()),
        ))
        .child(
            text(if detail.stopped_work {
                "Work stopped until this secret issue is resolved."
            } else {
                "Review this secret setup notice and resolve or dismiss it."
            })
            .text_size(px(12.0))
            .text_color(palette::text_muted()),
        )
        .child(actions)
}

fn loop_exhausted_detail_section(
    req: &AttentionRequest,
    detail: &LoopExhaustedDetail,
) -> impl IntoElement {
    let note_draft = store::inbox_note_draft();

    let mut attempts_col = view().gap(px(6.0));
    for attempt in &detail.attempts {
        attempts_col = attempts_col.child(
            view()
                .px(px(11.0))
                .py(px(7.0))
                .rounded(px(7.0))
                .bg(palette::subtle_surface_bg())
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(format!("Attempt {}", attempt.attempt))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .tabular_nums()
                        .text_color(if attempt.failed {
                            palette::destructive_text()
                        } else {
                            palette::success_text()
                        }),
                )
                .child(
                    text(attempt.summary.clone())
                        .flex_1()
                        .text_size(px(12.0))
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    let failure_card = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .px(px(14.0))
                .py(px(9.0))
                .bg(palette::pill_error_bg())
                .border_b_1()
                .border_color(palette::border_color())
                .child(
                    text(format!(
                        "Last failure ({} of {} attempts)",
                        detail.attempts_used, detail.max_attempts
                    ))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::pill_error_text()),
                ),
        )
        .child(
            view()
                .p(px(14.0))
                .gap(px(12.0))
                .child(
                    text(detail.last_error.clone())
                        .app_class("font-mono")
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                )
                .child(attempts_col),
        );

    let actions = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .child(
            button()
                .test_id("inbox-grant-attempts")
                .accessibility_label("Grant 3 More Attempts")
                .on_press(store::inbox_grant_more_attempts)
                .h(px(34.0))
                .px(px(16.0))
                .rounded(px(8.0))
                .bg(palette::nav_active_bg())
                .items_center()
                .justify_center()
                .active(|s| s.opacity(0.85))
                .child(
                    text("Grant 3 More Attempts")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        )
        .child(
            button()
                .test_id("inbox-stop-loop-run")
                .accessibility_label("Stop Run")
                .on_press(store::inbox_stop_exhausted_run)
                .h(px(34.0))
                .px(px(16.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::pill_error_bg()))
                .child(
                    text("Stop Run")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::destructive_text()),
                ),
        );

    view()
        .gap(px(16.0))
        .child(failure_card)
        .child(actions)
        .child(note_to_agent_section(
            "(guidance for next attempt, optional)",
            &req.note_placeholder,
            note_draft,
        ))
}

// --- Shared Note Input, Waiting Rules, Footer Bars --------------------------------------

fn note_to_agent_section(hint: &str, placeholder: &str, value: String) -> impl IntoElement {
    view()
        .gap(px(6.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .child(
                    text("Note to the agent")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(hint.to_owned())
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            input()
                .test_id("inbox-note-input")
                .placeholder(placeholder.to_owned())
                .value(value)
                .on_change(|next: &str| store::set_inbox_note_draft(next.to_owned()))
                .h(px(36.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .text_size(px(13.0))
                .text_color(palette::text_primary()),
        )
}

fn waiting_rules_panel() -> impl IntoElement {
    let rules = store::inbox_state().waiting_rules;
    view()
        .flex_shrink_0()
        .mx(px(20.0))
        .mb(px(10.0))
        .p(px(14.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::subtle_surface_bg())
        .gap(px(6.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("Waiting rules")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .test_id("inbox-waiting-rules-close")
                        .accessibility_label("Close Waiting rules")
                        .on_press(store::toggle_inbox_waiting_rules)
                        .w(px(22.0))
                        .h(px(22.0))
                        .rounded(px(5.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::sidebar_hover_bg()))
                        .child(icons::close_x(palette::text_muted())),
                ),
        )
        .child(
            text(format!(
                "• Menu bar badge turns orange and sends a silent reminder after {} min.",
                rules.reminder_after_minutes
            ))
            .text_size(px(12.0))
            .text_color(palette::text_secondary()),
        )
        .child(
            text(format!(
                "• Unanswered permissions auto-deny after {} min; questions let the agent decide after {} min.",
                rules.permission_timeout_minutes, rules.question_timeout_minutes
            ))
            .text_size(px(12.0))
            .text_color(palette::text_secondary()),
        )
        .child(
            text(if rules.review_waits_indefinitely {
                "• Review gates never auto-approve and wait until you respond."
            } else {
                "• Review gates follow the default timeout."
            })
            .text_size(px(12.0))
            .text_color(palette::text_secondary()),
        )
}

fn detail_timeout_bar(timeout_footer: &str) -> impl IntoElement {
    view()
        .h(px(40.0))
        .flex_shrink_0()
        .px(px(22.0))
        .border_t_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .child(icons::clock_progress(13.5, palette::text_muted()))
                .child(
                    text(timeout_footer.to_owned())
                        .text_size(px(12.0))
                        .text_color(palette::text_muted())
                        .truncate(),
                ),
        )
        .child(
            button()
                .test_id("inbox-waiting-rules-btn")
                .accessibility_label("Waiting rules")
                .on_press(store::toggle_inbox_waiting_rules)
                .px(px(4.0))
                .py(px(2.0))
                .active(|s| s.opacity(0.75))
                .child(
                    text("Waiting rules")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::link_blue()),
                ),
        )
}

fn inbox_shortcut_footer() -> impl IntoElement {
    view()
        .h(px(32.0))
        .flex_shrink_0()
        .px(px(14.0))
        .bg(palette::header_bg())
        .border_t_1()
        .border_color(palette::border_color())
        .flex_row()
        .items_center()
        .gap(px(16.0))
        .child(shortcut_hint("↑↓", "Move between requests"))
        .child(shortcut_hint("⌘↩", "Primary answer"))
        .child(shortcut_hint("⌥⌘↩", "Allow for this run"))
        .child(shortcut_hint("⌘⌫", "Deny"))
        .child(shortcut_hint("E", "Edit before allowing"))
        .child(shortcut_hint("O", "Open run"))
}

fn shortcut_hint(keys: &'static str, label: &'static str) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(
            view()
                .px(px(5.5))
                .py(px(1.5))
                .rounded(px(4.5))
                .border_1()
                .border_color(palette::inbox_keycap_border())
                .bg(palette::inbox_keycap_bg())
                .child(
                    text(keys)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(
            text(label)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}
