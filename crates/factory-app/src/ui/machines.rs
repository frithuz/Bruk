//! Machines view: SSH worker fleet master-detail split, schema compatibility pills,
//! catch-up event banners, machine metadata cards, live sessions table, and inline
//! Update Cores / Add Machine / Machine settings drawers matching `21 Machines.png`.

use factory_core::{
    MachineCatchUpSummary, MachineConnectionStatus, MachinePlatform, MachineRecord, MachineSession,
    MachinesFleet, NewMachineDraft, SchemaCompatibility, SessionNowTone,
};
use nsh_base::{button, input};
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{icons, palette},
};

const MACHINE_LIST_WIDTH: f32 = 312.0;

/// Header toolbar controls shown on the right when `ActiveView::Machines` is active.
pub(crate) fn machines_header_controls() -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .window_drag(false)
        .child(
            button()
                .test_id("machines-header-update-cores")
                .accessibility_label("Update Cores...")
                .on_press(|| {
                    let open = store::machines_update_cores_open();
                    store::set_machines_update_cores_open(!open);
                })
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
                .child(icons::download_tray(palette::text_secondary()))
                .child(
                    text("Update Cores...")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .test_id("machines-header-add")
                .accessibility_label("Add Machine...")
                .on_press(|| {
                    let open = store::machines_add_drawer_open();
                    store::set_machines_add_drawer_open(!open);
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
                    text("Add Machine...")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        )
}

/// Master-detail Machines canvas: scrollable machine list on the left, selected machine detail on the right.
pub(crate) fn machines_view() -> impl IntoElement {
    let draft_name = signal(String::new());
    let draft_platform = signal(MachinePlatform::Linux);
    let draft_ssh = signal(String::new());
    let draft_address = signal(String::new());
    let draft_error = signal(Option::<String>::None);

    view()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .flex_row()
        .bg(palette::canvas_bg())
        .child(machine_list_pane())
        .child(machine_detail_pane(
            draft_name,
            draft_platform,
            draft_ssh,
            draft_address,
            draft_error,
        ))
}

// --- Left Machine List Pane -------------------------------------------------------------

fn machine_list_pane() -> impl IntoElement {
    view()
        .w(px(MACHINE_LIST_WIDTH))
        .flex_shrink_0()
        .min_h(px(0.0))
        .bg(palette::canvas_bg())
        .border_r_1()
        .border_color(palette::border_color())
        .child(scroll().flex_1().child(dynamic(|| {
            let fleet = store::machines_fleet();
            let selected_id = store::selected_machine_id();

            let mut list = view().flex_1().p(px(14.0)).gap(px(10.0));
            for machine in &fleet.machines {
                let is_selected = selected_id == Some(machine.id);
                list = list.child(machine_list_card(machine, fleet.bruk_schema, is_selected));
            }

            list = list.child(
                view().px(px(4.0)).pt(px(6.0)).pb(px(10.0)).child(
                    text(
                        "Machines connect over SSH with a key. Sessions keep running on a machine when Bruk disconnects, and catch up when it reconnects. On every connection Bruk and the core compare data schemas; version numbers only matter when the schema differs.",
                    )
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                ),
            );

            list
        })))
}

fn machine_list_card(
    machine: &MachineRecord,
    bruk_schema: u32,
    is_selected: bool,
) -> impl IntoElement {
    let id = machine.id;
    let slug = slugify(&machine.name);
    let test_id = format!("machine-card-{slug}");
    let compat = machine.compatibility(bruk_schema);
    let badge_bg = if is_selected {
        palette::machine_badge_selected_bg()
    } else {
        palette::machine_badge_bg()
    };

    let mut content = view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(4.5))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    text(machine.name.clone())
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(machine.session_count_label())
                        .text_size(px(11.5))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            text(machine.os_short.clone())
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .pt(px(2.0))
                .child(status_icon(machine.status))
                .child(
                    text(machine.status.label())
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(status_text_color(machine.status)),
                )
                .child(
                    text(machine.status_detail.clone())
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .child(
                    text("Core")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(machine.core_version.clone())
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .tabular_nums()
                        .text_color(palette::text_secondary()),
                )
                .child(
                    text(format!("schema {}", machine.core_schema))
                        .text_size(px(11.5))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                ),
        );

    if let Some(badge) = compat.badge_label() {
        let (bg, border, fg) = match compat {
            SchemaCompatibility::CoreUpdateSuggested => (
                palette::pill_warning_bg(),
                palette::pill_warning_border(),
                palette::pill_warning_text(),
            ),
            SchemaCompatibility::NeedsNewerBruk => (
                palette::pill_error_bg(),
                palette::pill_error_border(),
                palette::pill_error_text(),
            ),
            SchemaCompatibility::InSync => unreachable!(),
        };
        content = content.child(
            view().pt(px(2.0)).flex_row().child(
                view()
                    .px(px(8.0))
                    .py(px(2.5))
                    .rounded_full()
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .child(
                        text(badge)
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg),
                    ),
            ),
        );
    }

    let mut card = button()
        .test_id(&test_id)
        .accessibility_label(format!("Select machine {}", machine.name))
        .on_press(move || store::select_machine(id))
        .rounded(px(11.0))
        .px(px(12.0))
        .py(px(11.0))
        .flex_row()
        .items_start()
        .gap(px(10.0))
        .bg(if is_selected {
            palette::card_selected_bg()
        } else {
            palette::card_bg()
        })
        .hover(move |s| {
            if is_selected {
                s
            } else {
                s.bg(palette::card_inset_bg())
            }
        })
        .active(|s| s.opacity(0.9))
        .child(
            view()
                .w(px(28.0))
                .h(px(26.0))
                .mt(px(1.0))
                .rounded(px(6.0))
                .bg(badge_bg)
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(
                    text(machine.platform.short_badge())
                        .app_class("font-mono")
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(content);

    card = if is_selected {
        card.border_2()
            .border_color(palette::card_selected_border())
    } else {
        card.border_1().border_color(palette::border_color())
    };

    card
}

// --- Right Machine Detail Pane ----------------------------------------------------------

fn machine_detail_pane(
    draft_name: Signal<String>,
    draft_platform: Signal<MachinePlatform>,
    draft_ssh: Signal<String>,
    draft_address: Signal<String>,
    draft_error: Signal<Option<String>>,
) -> impl IntoElement {
    scroll()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .bg(palette::card_bg())
        .child(dynamic(move || {
            let fleet = store::machines_fleet();
            let update_cores_open = store::machines_update_cores_open();
            let add_open = store::machines_add_drawer_open();
            let selected = store::selected_machine();
            let changes_expanded = store::machines_changes_expanded();
            let settings_open = store::machines_settings_open();

            let mut container = view().flex_1().px(px(28.0)).py(px(22.0)).gap(px(18.0));

            if update_cores_open {
                container = container.child(update_cores_drawer(&fleet));
            }

            if add_open {
                container = container.child(add_machine_drawer(
                    draft_name,
                    draft_platform,
                    draft_ssh,
                    draft_address,
                    draft_error,
                ));
            }

            if let Some(machine) = selected {
                container = container.child(machine_detail_header(&machine));

                if let Some(banner) =
                    machine_status_banner(&machine, fleet.bruk_schema, &fleet.bruk_version, changes_expanded)
                {
                    container = container.child(banner);
                }

                container = container
                    .child(machine_info_cards_row(&machine, fleet.bruk_schema))
                    .child(machine_sessions_section(&machine))
                    .child(machine_footer_row(&machine, settings_open));

                if settings_open {
                    container = container.child(machine_settings_drawer(&machine));
                }
            } else {
                container = container.child(
                    view()
                        .p(px(24.0))
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .child(
                            text("No machines configured yet. Add an SSH host to run sessions remotely.")
                                .text_size(px(13.0))
                                .text_color(palette::text_muted()),
                        ),
                );
            }

            container
        }))
}

fn machine_detail_header(machine: &MachineRecord) -> impl IntoElement {
    let id = machine.id;
    let (pill_bg, pill_fg) = status_pill_colors(machine.status);

    let mut actions = view().flex_row().items_center().gap(px(8.0)).child(
        button()
            .test_id("machine-action-terminal")
            .accessibility_label("Open in Terminal")
            .on_press(move || store::open_machine_in_terminal(id))
            .h(px(30.0))
            .px(px(12.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::control_bg())
            .items_center()
            .justify_center()
            .hover(|s| s.bg(palette::subtle_surface_bg()))
            .active(|s| s.opacity(0.8))
            .child(
                text("Open in Terminal")
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_primary()),
            ),
    );

    if !machine.is_local {
        let is_offline = machine.status.is_offline();
        let label = if is_offline {
            "Reconnect"
        } else {
            "Disconnect"
        };
        actions = actions.child(
            button()
                .test_id("machine-action-connection")
                .accessibility_label(label)
                .on_press(move || {
                    if is_offline {
                        store::reconnect_machine(id);
                    } else {
                        store::disconnect_machine(id);
                    }
                })
                .h(px(30.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .items_center()
                .justify_center()
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text(label)
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        );
    }

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(machine.name.clone())
                        .text_size(px(21.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .px(px(10.0))
                        .py(px(3.0))
                        .rounded_full()
                        .bg(pill_bg)
                        .child(
                            text(machine.status.label())
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(pill_fg),
                        ),
                ),
        )
        .child(actions)
}

fn machine_status_banner(
    machine: &MachineRecord,
    bruk_schema: u32,
    bruk_version: &str,
    changes_expanded: bool,
) -> Option<AnyElement> {
    if let Some(catch_up) = &machine.catch_up {
        return Some(catch_up_banner(catch_up, changes_expanded).into_element());
    }

    let id = machine.id;
    let compat = machine.compatibility(bruk_schema);
    if compat == SchemaCompatibility::CoreUpdateSuggested {
        return Some(
            view()
                .rounded(px(10.0))
                .border_1()
                .border_color(palette::pill_warning_border())
                .bg(palette::callout_amber_bg())
                .px(px(14.0))
                .py(px(12.0))
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(14.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_start()
                        .gap(px(10.0))
                        .child(view().pt(px(1.5)).child(icons::alert_circle(palette::pill_warning_text())))
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(3.0))
                                .child(
                                    text(format!(
                                        "Core {} (schema {}) is behind Bruk {} (schema {})",
                                        machine.core_version,
                                        machine.core_schema,
                                        bruk_version,
                                        bruk_schema
                                    ))
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::pill_warning_text()),
                                )
                                .child(
                                    text(
                                        "Update the Bruk core binary on this host to match schema 7 so session metadata and catch-up events stay fully compatible.",
                                    )
                                    .text_size(px(12.0))
                                    .text_color(palette::text_secondary()),
                                ),
                        ),
                )
                .child(
                    button()
                        .test_id("machine-banner-update-core")
                        .accessibility_label("Update Core")
                        .on_press(move || store::update_machine_core(id))
                        .h(px(28.0))
                        .px(px(11.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Update Core")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .into_element(),
        );
    }

    if compat == SchemaCompatibility::NeedsNewerBruk {
        return Some(
            view()
                .rounded(px(10.0))
                .border_1()
                .border_color(palette::pill_error_border())
                .bg(palette::pill_error_bg())
                .px(px(14.0))
                .py(px(12.0))
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(14.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_start()
                        .gap(px(10.0))
                        .child(
                            view()
                                .pt(px(1.5))
                                .child(icons::status_not_syncing(palette::pill_error_text())),
                        )
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(3.0))
                                .child(
                                    text(format!(
                                        "Core {} uses data schema {}, which needs a newer Bruk",
                                        machine.core_version, machine.core_schema
                                    ))
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::pill_error_text()),
                                )
                                .child(
                                    text(format!(
                                        "SSH is connected, but syncing is paused because this Bruk ({bruk_version}) only supports up to schema {bruk_schema}."
                                    ))
                                    .text_size(px(12.0))
                                    .text_color(palette::text_secondary()),
                                ),
                        ),
                )
                .child(
                    button()
                        .test_id("machine-banner-check-bruk-update")
                        .accessibility_label("Check for Bruk Update")
                        .on_press(store::check_bruk_update)
                        .h(px(28.0))
                        .px(px(11.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Check for Bruk Update")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .into_element(),
        );
    }

    if machine.status == MachineConnectionStatus::Unreachable {
        return Some(
            view()
                .rounded(px(10.0))
                .border_1()
                .border_color(palette::pill_error_border())
                .bg(palette::pill_error_bg())
                .px(px(14.0))
                .py(px(12.0))
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(14.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_start()
                        .gap(px(10.0))
                        .child(
                            view()
                                .pt(px(1.5))
                                .child(icons::status_unreachable(palette::pill_error_text())),
                        )
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(3.0))
                                .child(
                                    text(format!(
                                        "Unable to reach {} over SSH ({})",
                                        machine.name, machine.status_detail
                                    ))
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::pill_error_text()),
                                )
                                .child(
                                    text(format!(
                                        "Connection attempts to {} ({}) timed out. Check that the machine is awake and reachable on your network.",
                                        machine.ssh_target, machine.address_primary
                                    ))
                                    .text_size(px(12.0))
                                    .text_color(palette::text_secondary()),
                                ),
                        ),
                )
                .child(
                    button()
                        .test_id("machine-banner-retry-ssh")
                        .accessibility_label("Test Connection")
                        .on_press(move || store::reconnect_machine(id))
                        .h(px(28.0))
                        .px(px(11.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Test Connection")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .into_element(),
        );
    }

    None
}

fn catch_up_banner(catch_up: &MachineCatchUpSummary, expanded: bool) -> impl IntoElement {
    let mut banner = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::banner_info_border())
        .bg(palette::intake_banner_bg())
        .px(px(14.0))
        .py(px(12.0))
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_start()
                .justify_between()
                .gap(px(14.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_start()
                        .gap(px(10.0))
                        .child(
                            view()
                                .pt(px(1.5))
                                .child(icons::info_circle(palette::link_blue())),
                        )
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(3.5))
                                .child(
                                    text(catch_up.headline.clone())
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::link_blue()),
                                )
                                .child(
                                    text(catch_up.summary.clone())
                                        .text_size(px(12.0))
                                        .text_color(palette::text_secondary()),
                                ),
                        ),
                )
                .child(
                    button()
                        .test_id("machine-banner-show-changes")
                        .accessibility_label(if expanded {
                            "Hide Changes"
                        } else {
                            "Show What Changed"
                        })
                        .on_press(store::toggle_machines_changes_expanded)
                        .h(px(28.0))
                        .px(px(11.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text(if expanded {
                                "Hide Changes"
                            } else {
                                "Show What Changed"
                            })
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_primary()),
                        ),
                ),
        );

    if expanded && !catch_up.changes.is_empty() {
        let mut details = view()
            .pt(px(8.0))
            .pl(px(25.0))
            .border_t_1()
            .border_color(palette::intake_banner_border())
            .gap(px(4.5));
        for change in &catch_up.changes {
            details = details.child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(7.0))
                    .child(
                        view()
                            .w(px(4.5))
                            .h(px(4.5))
                            .rounded_full()
                            .bg(palette::link_blue())
                            .flex_shrink_0(),
                    )
                    .child(
                        text(change.clone())
                            .text_size(px(11.5))
                            .text_color(palette::text_secondary()),
                    ),
            );
        }
        banner = banner.child(details);
    }

    banner
}

fn machine_info_cards_row(machine: &MachineRecord, bruk_schema: u32) -> impl IntoElement {
    let core_note_color = match machine.compatibility(bruk_schema) {
        SchemaCompatibility::InSync => palette::text_muted(),
        SchemaCompatibility::CoreUpdateSuggested => palette::pill_warning_text(),
        SchemaCompatibility::NeedsNewerBruk => palette::pill_error_text(),
    };

    view()
        .flex_row()
        .gap(px(12.0))
        .child(info_metric_card(
            "System",
            &machine.os_detail,
            false,
            &machine.hardware_spec,
            false,
            palette::text_muted(),
        ))
        .child(info_metric_card(
            "Address",
            &machine.address_primary,
            true,
            &machine.address_secondary,
            true,
            palette::text_muted(),
        ))
        .child(info_metric_card(
            "SSH",
            &machine.ssh_target,
            true,
            &machine.ssh_key_fingerprint,
            true,
            palette::text_muted(),
        ))
        .child(info_metric_card(
            "Bruk core",
            &machine.core_summary_label(),
            true,
            &machine.core_sync_note,
            false,
            core_note_color,
        ))
}

fn info_metric_card(
    label: &str,
    primary: &str,
    primary_mono: bool,
    secondary: &str,
    secondary_mono: bool,
    secondary_color: AdaptiveColor,
) -> impl IntoElement {
    let mut primary_el = text(primary.to_owned())
        .text_size(px(13.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::text_primary());
    if primary_mono {
        primary_el = primary_el.app_class("font-mono").tabular_nums();
    }

    let mut secondary_el = text(secondary.to_owned())
        .text_size(px(11.5))
        .text_color(secondary_color);
    if secondary_mono {
        secondary_el = secondary_el.app_class("font-mono").tabular_nums();
    }

    view()
        .flex_1()
        .min_w(px(0.0))
        .px(px(14.0))
        .py(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(4.0))
        .child(
            text(label.to_owned())
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(primary_el)
        .child(secondary_el)
}

fn machine_sessions_section(machine: &MachineRecord) -> impl IntoElement {
    let mut table = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .flex_row()
                .items_center()
                .px(px(14.0))
                .py(px(9.0))
                .bg(palette::table_header_bg())
                .border_b_1()
                .border_color(palette::divider_color())
                .child(
                    text("Run")
                        .w(px(64.0))
                        .flex_shrink_0()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                )
                .child(
                    text("Project › workflow")
                        .flex_1()
                        .min_w(px(0.0))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                )
                .child(
                    text("Now")
                        .w(px(188.0))
                        .flex_shrink_0()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                )
                .child(
                    text("Link")
                        .w(px(116.0))
                        .flex_shrink_0()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                )
                .child(view().w(px(52.0)).flex_shrink_0()),
        );

    if machine.sessions.is_empty() {
        table = table.child(
            view().px(px(14.0)).py(px(18.0)).child(
                text(format!("No active sessions on {}.", machine.name))
                    .text_size(px(12.5))
                    .text_color(palette::text_muted()),
            ),
        );
    } else {
        for (idx, session) in machine.sessions.iter().enumerate() {
            table = table.child(session_table_row(session, idx > 0));
        }
    }

    view()
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    text("Sessions")
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(machine.sessions_subtitle.clone())
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(table)
}

fn session_table_row(session: &MachineSession, top_divider: bool) -> impl IntoElement {
    let run_number = session.run_number;
    let project_name = session.project_name.clone();
    let workflow_name = session.workflow_name.clone();
    let test_id = format!("machine-session-open-{run_number}");

    let now_color = match session.now_tone {
        SessionNowTone::Attention => palette::workflow_now_amber(),
        SessionNowTone::Active => palette::link_blue(),
        SessionNowTone::Error => palette::destructive_text(),
        SessionNowTone::Muted => palette::text_muted(),
    };

    let mut row = view()
        .flex_row()
        .items_center()
        .px(px(14.0))
        .py(px(10.5))
        .bg(palette::card_bg());

    if top_divider {
        row = row.border_t_1().border_color(palette::divider_color());
    }

    row.child(
        text(session.run_tag())
            .w(px(64.0))
            .flex_shrink_0()
            .app_class("font-mono")
            .text_size(px(12.0))
            .tabular_nums()
            .text_color(palette::text_muted()),
    )
    .child(
        text(session.project_workflow_label())
            .flex_1()
            .min_w(px(0.0))
            .text_size(px(12.5))
            .text_color(palette::text_primary()),
    )
    .child(
        text(session.now_label.clone())
            .w(px(188.0))
            .flex_shrink_0()
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(now_color),
    )
    .child(
        text(session.link_label.clone())
            .w(px(116.0))
            .flex_shrink_0()
            .text_size(px(12.0))
            .text_color(palette::text_muted()),
    )
    .child(
        view()
            .w(px(52.0))
            .flex_shrink_0()
            .flex_row()
            .justify_end()
            .child(
                button()
                    .test_id(&test_id)
                    .accessibility_label(format!(
                        "Open run #{} {} {}",
                        run_number, project_name, workflow_name
                    ))
                    .on_press(move || {
                        store::open_machine_session(&project_name, &workflow_name, run_number);
                    })
                    .hover(|s| s.opacity(0.75))
                    .active(|s| s.opacity(0.6))
                    .child(
                        view()
                            .border_b_1()
                            .border_color(palette::link_blue())
                            .child(
                                text("Open")
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::link_blue()),
                            ),
                    ),
            ),
    )
}

fn machine_footer_row(machine: &MachineRecord, settings_open: bool) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            button()
                .test_id("machine-settings-toggle")
                .accessibility_label(if settings_open {
                    "Close machine settings"
                } else {
                    "Machine settings..."
                })
                .on_press(store::toggle_machines_settings_open)
                .hover(|s| s.opacity(0.75))
                .active(|s| s.opacity(0.6))
                .child(
                    view()
                        .border_b_1()
                        .border_color(palette::link_blue())
                        .child(
                            text(if settings_open {
                                "Close settings"
                            } else {
                                "Machine settings..."
                            })
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::link_blue()),
                        ),
                ),
        )
        .child(
            text(format!(
                "Projects that can run here: {}",
                machine.allowed_projects_summary()
            ))
            .text_size(px(12.0))
            .text_color(palette::text_muted()),
        )
}

// --- Inline Drawers ---------------------------------------------------------------------

fn update_cores_drawer(fleet: &MachinesFleet) -> impl IntoElement {
    let outdated: Vec<&MachineRecord> = fleet
        .machines
        .iter()
        .filter(|m| m.compatibility(fleet.bruk_schema) == SchemaCompatibility::CoreUpdateSuggested)
        .collect();
    let newer: Vec<&MachineRecord> = fleet
        .machines
        .iter()
        .filter(|m| m.compatibility(fleet.bruk_schema) == SchemaCompatibility::NeedsNewerBruk)
        .collect();

    let mut card = view()
        .rounded(px(11.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    view()
                        .gap(px(2.0))
                        .child(
                            text("Update Bruk Cores")
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(format!(
                                "Target binary: Bruk core {} (data schema {})",
                                fleet.bruk_version, fleet.bruk_schema
                            ))
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    button()
                        .test_id("machines-update-cores-close")
                        .accessibility_label("Close Update Cores")
                        .on_press(|| store::set_machines_update_cores_open(false))
                        .w(px(24.0))
                        .h(px(24.0))
                        .rounded(px(6.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::close_x(palette::text_muted())),
                ),
        );

    if outdated.is_empty() {
        card = card.child(
            text("All reachable machines are already running schema 7 or newer.")
                .text_size(px(12.0))
                .text_color(palette::success_text()),
        );
    } else {
        let mut rows = view().gap(px(8.0));
        for machine in &outdated {
            let id = machine.id;
            let slug = slugify(&machine.name);
            rows = rows.child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(12.0))
                    .py(px(9.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_bg())
                    .child(
                        view()
                            .gap(px(2.0))
                            .child(
                                text(machine.name.clone())
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::text_primary()),
                            )
                            .child(
                                text(format!(
                                    "Core {} (schema {}) → {} (schema {})",
                                    machine.core_version,
                                    machine.core_schema,
                                    fleet.bruk_version,
                                    fleet.bruk_schema
                                ))
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                            ),
                    )
                    .child(
                        button()
                            .test_id(&format!("machines-update-core-{slug}"))
                            .accessibility_label(format!("Update core on {}", machine.name))
                            .on_press(move || store::update_machine_core(id))
                            .h(px(26.0))
                            .px(px(10.0))
                            .rounded(px(6.5))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::control_bg())
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(palette::subtle_surface_bg()))
                            .child(
                                text("Update")
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_primary()),
                            ),
                    ),
            );
        }
        card = card.child(rows).child(
            view().flex_row().justify_end().child(
                button()
                    .test_id("machines-update-all-cores")
                    .accessibility_label("Update All Suggested Cores")
                    .on_press(store::update_all_suggested_cores)
                    .h(px(28.0))
                    .px(px(12.0))
                    .rounded(px(7.0))
                    .bg(palette::nav_active_bg())
                    .items_center()
                    .justify_center()
                    .active(|s| s.opacity(0.85))
                    .child(
                        text("Update All Suggested")
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::nav_active_fg()),
                    ),
            ),
        );
    }

    if !newer.is_empty() {
        let names: Vec<String> = newer.iter().map(|m| m.name.clone()).collect();
        card = card.child(
            text(format!(
                "Skipped (newer schema — update Bruk app instead): {}",
                names.join(", ")
            ))
            .text_size(px(11.5))
            .text_color(palette::text_muted()),
        );
    }

    card
}

fn add_machine_drawer(
    draft_name: Signal<String>,
    draft_platform: Signal<MachinePlatform>,
    draft_ssh: Signal<String>,
    draft_address: Signal<String>,
    draft_error: Signal<Option<String>>,
) -> impl IntoElement {
    let platform = draft_platform.get();
    let error = draft_error.get();

    let mut card = view()
        .rounded(px(11.0))
        .border_1()
        .border_color(palette::card_selected_border())
        .bg(palette::card_inset_bg())
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    view()
                        .gap(px(2.0))
                        .child(
                            text("Add SSH Machine")
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text("Connect a remote host over SSH with a key and install Bruk core 0.9.0.")
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    button()
                        .test_id("machines-add-close")
                        .accessibility_label("Close Add Machine")
                        .on_press(|| store::set_machines_add_drawer_open(false))
                        .w(px(24.0))
                        .h(px(24.0))
                        .rounded(px(6.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::close_x(palette::text_muted())),
                ),
        )
        .child(
            view()
                .flex_row()
                .gap(px(10.0))
                .child(
                    view()
                        .flex_1()
                        .gap(px(4.0))
                        .child(
                            text("Machine name")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_secondary()),
                        )
                        .child(
                            view()
                                .h(px(30.0))
                                .px(px(9.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .justify_center()
                                .child(
                                    input()
                                        .test_id("machines-add-name")
                                        .value(move || draft_name.get())
                                        .on_change(move |v: &str| {
                                            draft_name.set(v.to_owned());
                                            draft_error.set(None);
                                        })
                                        .placeholder("build-box-02")
                                        .bg(AdaptiveColor::TRANSPARENT)
                                        .text_size(px(12.5))
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                )
                .child(
                    view()
                        .flex_1()
                        .gap(px(4.0))
                        .child(
                            text("SSH target")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_secondary()),
                        )
                        .child(
                            view()
                                .h(px(30.0))
                                .px(px(9.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .justify_center()
                                .child(
                                    input()
                                        .test_id("machines-add-ssh")
                                        .value(move || draft_ssh.get())
                                        .on_change(move |v: &str| {
                                            draft_ssh.set(v.to_owned());
                                            draft_error.set(None);
                                        })
                                        .placeholder("bruk@build-box-02")
                                        .bg(AdaptiveColor::TRANSPARENT)
                                        .text_size(px(12.5))
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                )
                .child(
                    view()
                        .flex_1()
                        .gap(px(4.0))
                        .child(
                            text("Address")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_secondary()),
                        )
                        .child(
                            view()
                                .h(px(30.0))
                                .px(px(9.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .justify_center()
                                .child(
                                    input()
                                        .test_id("machines-add-address")
                                        .value(move || draft_address.get())
                                        .on_change(move |v: &str| {
                                            draft_address.set(v.to_owned());
                                        })
                                        .placeholder("10.0.4.55:22")
                                        .bg(AdaptiveColor::TRANSPARENT)
                                        .text_size(px(12.5))
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text("Platform")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_secondary()),
                        )
                        .child(platform_chip(MachinePlatform::Linux, platform, draft_platform))
                        .child(platform_chip(MachinePlatform::Mac, platform, draft_platform))
                        .child(platform_chip(MachinePlatform::Windows, platform, draft_platform)),
                )
                .child(
                    button()
                        .test_id("machines-add-submit")
                        .accessibility_label("Connect Machine")
                        .on_press(move || {
                            let draft = NewMachineDraft {
                                name: draft_name.get(),
                                platform: draft_platform.get(),
                                os_label: String::new(),
                                arch: String::new(),
                                address: draft_address.get(),
                                ssh_target: draft_ssh.get(),
                                allowed_projects: vec![
                                    "api-gateway".to_owned(),
                                    "marketing-site".to_owned(),
                                    "mobile-app".to_owned(),
                                ],
                            };
                            match store::add_machine(draft) {
                                Ok(()) => {
                                    draft_name.set(String::new());
                                    draft_ssh.set(String::new());
                                    draft_address.set(String::new());
                                    draft_error.set(None);
                                }
                                Err(err) => draft_error.set(Some(err)),
                            }
                        })
                        .h(px(28.0))
                        .px(px(12.0))
                        .rounded(px(7.0))
                        .bg(palette::nav_active_bg())
                        .items_center()
                        .justify_center()
                        .active(|s| s.opacity(0.85))
                        .child(
                            text("Connect Machine")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        );

    if let Some(err) = error {
        card = card.child(
            text(err)
                .text_size(px(11.5))
                .text_color(palette::destructive_text()),
        );
    }

    card
}

fn platform_chip(
    platform: MachinePlatform,
    active: MachinePlatform,
    draft_platform: Signal<MachinePlatform>,
) -> impl IntoElement {
    let selected = platform == active;
    button()
        .test_id(&format!("machines-platform-{}", platform.short_badge()))
        .accessibility_label(platform.display_name())
        .on_press(move || draft_platform.set(platform))
        .h(px(26.0))
        .px(px(10.0))
        .rounded(px(6.5))
        .border_1()
        .border_color(if selected {
            palette::card_selected_border()
        } else {
            palette::border_color()
        })
        .bg(if selected {
            palette::card_selected_bg()
        } else {
            palette::control_bg()
        })
        .items_center()
        .justify_center()
        .child(
            text(platform.display_name())
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if selected {
                    palette::link_blue()
                } else {
                    palette::text_secondary()
                }),
        )
}

fn machine_settings_drawer(machine: &MachineRecord) -> impl IntoElement {
    let id = machine.id;
    let available_projects = ["api-gateway", "marketing-site", "mobile-app"];

    let mut chips = view().flex_row().items_center().gap(px(8.0));
    for project in available_projects {
        let allowed = machine.allowed_projects.iter().any(|p| p == project);
        let project_owned = project.to_owned();
        let slug = slugify(project);
        chips = chips.child(
            button()
                .test_id(&format!("machine-setting-project-{slug}"))
                .accessibility_label(format!("Toggle project {project}"))
                .on_press(move || store::toggle_machine_project_access(id, &project_owned))
                .h(px(28.0))
                .px(px(10.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(if allowed {
                    palette::card_selected_border()
                } else {
                    palette::border_color()
                })
                .bg(if allowed {
                    palette::card_selected_bg()
                } else {
                    palette::control_bg()
                })
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .child(
                    text(if allowed { "✓" } else { "+" })
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if allowed {
                            palette::link_blue()
                        } else {
                            palette::text_muted()
                        }),
                )
                .child(
                    text(project)
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if allowed {
                            palette::link_blue()
                        } else {
                            palette::text_secondary()
                        }),
                ),
        );
    }

    let mut card = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(format!("Machine settings · {}", machine.name))
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .test_id("machine-settings-close")
                        .accessibility_label("Close Machine Settings")
                        .on_press(store::toggle_machines_settings_open)
                        .w(px(24.0))
                        .h(px(24.0))
                        .rounded(px(6.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::close_x(palette::text_muted())),
                ),
        )
        .child(
            view()
                .gap(px(6.0))
                .child(
                    text("Allowed projects")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                )
                .child(chips),
        );

    if !machine.is_local {
        card = card.child(
            view()
                .pt(px(8.0))
                .border_t_1()
                .border_color(palette::divider_color())
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text("Remove this remote host from Bruk's fleet list.")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .child(
                    button()
                        .test_id("machine-settings-remove")
                        .accessibility_label("Remove Machine")
                        .on_press(move || store::remove_machine(id))
                        .h(px(28.0))
                        .px(px(11.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::pill_error_border())
                        .bg(palette::pill_error_bg())
                        .items_center()
                        .justify_center()
                        .hover(|s| s.opacity(0.85))
                        .child(
                            text("Remove Machine")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::pill_error_text()),
                        ),
                ),
        );
    }

    card
}

// --- Visual Helpers ---------------------------------------------------------------------

fn status_icon(status: MachineConnectionStatus) -> AnyElement {
    let color = status_text_color(status);
    match status {
        MachineConnectionStatus::Connected => icons::status_merged(color).into_element(),
        MachineConnectionStatus::Disconnected => icons::status_disconnected(color).into_element(),
        MachineConnectionStatus::NotSyncing => icons::status_not_syncing(color).into_element(),
        MachineConnectionStatus::Unreachable => icons::status_unreachable(color).into_element(),
    }
}

fn status_text_color(status: MachineConnectionStatus) -> AdaptiveColor {
    match status {
        MachineConnectionStatus::Connected => palette::success_text(),
        MachineConnectionStatus::Disconnected => palette::workflow_now_amber(),
        MachineConnectionStatus::NotSyncing | MachineConnectionStatus::Unreachable => {
            palette::destructive_text()
        }
    }
}

fn status_pill_colors(status: MachineConnectionStatus) -> (AdaptiveColor, AdaptiveColor) {
    match status {
        MachineConnectionStatus::Connected => {
            (palette::status_success_bg(), palette::success_text())
        }
        MachineConnectionStatus::Disconnected => {
            (palette::status_warning_bg(), palette::workflow_now_amber())
        }
        MachineConnectionStatus::NotSyncing | MachineConnectionStatus::Unreachable => {
            (palette::status_error_bg(), palette::destructive_text())
        }
    }
}

fn slugify(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "-")
}
