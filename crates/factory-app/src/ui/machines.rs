//! Machines view displaying local and remote worker hosts executing factory runs.

use factory_core::{MachineNode, MachineStatus};
use nsh_base::button;
use nsh_core::prelude::*;

use crate::{store, ui::palette};

pub(crate) fn machines_view() -> impl IntoElement {
    scroll()
        .flex_1()
        .bg(palette::canvas_bg())
        .child(dynamic(|| {
            let machines = store::machines();
            let mut container = view().flex_1().p(px(18.0)).gap(px(10.0));
            for machine in machines {
                container = container.child(machine_card(&machine));
            }
            container
        }))
}

fn machine_card(machine: &MachineNode) -> impl IntoElement {
    let id = machine.id;
    let is_connected = machine.status == MachineStatus::Connected;
    let status_color = if is_connected {
        palette::success_text()
    } else {
        palette::warning_text()
    };

    view()
        .p(px(16.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(4.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(view().w(px(8.0)).h(px(8.0)).rounded_full().bg(status_color))
                        .child(
                            text(machine.name.clone())
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(machine.status.label())
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(status_color),
                        ),
                )
                .child(
                    text(machine.spec.clone())
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(format!(
                        "{}/{} agent slots in use · {}",
                        machine.active_runs, machine.max_slots, machine.last_seen
                    ))
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                ),
        )
        .child(if is_connected {
            view()
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(7.0))
                .bg(palette::subtle_surface_bg())
                .child(
                    text("Online")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                )
                .into_element()
        } else {
            button()
                .on_press(move || store::reconnect_machine(id))
                .px(px(12.0))
                .py(px(6.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Reconnect")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .into_element()
        })
}
