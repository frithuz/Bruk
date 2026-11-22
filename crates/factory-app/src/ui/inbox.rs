//! Inbox view showing pending permissions, questions, and review gates waiting on the operator.

use factory_core::{InboxItem, ProjectColor, format_wait_minutes};
use nsh_base::button;
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{dashboard::project_dot, palette},
};

pub(crate) fn inbox_view() -> impl IntoElement {
    scroll()
        .flex_1()
        .bg(palette::canvas_bg())
        .child(dynamic(|| {
            let items = store::inbox_items();
            let pending: Vec<InboxItem> = items.iter().filter(|i| !i.resolved).cloned().collect();
            let resolved: Vec<InboxItem> = items.iter().filter(|i| i.resolved).cloned().collect();

            let mut list = view().flex_1().p(px(18.0)).gap(px(14.0));

            if pending.is_empty() {
                list = list.child(
                    view()
                        .p(px(20.0))
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .gap(px(4.0))
                        .child(
                            text("All caught up")
                                .text_size(px(14.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text("No agents are currently waiting on your input.")
                                .text_size(px(12.5))
                                .text_color(palette::text_muted()),
                        ),
                );
            } else {
                let mut pending_col = view().gap(px(10.0));
                for item in pending {
                    pending_col = pending_col.child(inbox_card(&item));
                }
                list = list.child(pending_col);
            }

            if !resolved.is_empty() {
                let mut resolved_col = view().gap(px(8.0)).child(
                    text("Answered recently")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                );
                for item in resolved {
                    resolved_col = resolved_col.child(inbox_card(&item));
                }
                list = list.child(resolved_col);
            }

            list
        }))
}

fn inbox_card(item: &InboxItem) -> impl IntoElement {
    let id = item.id;
    let resolved = item.resolved;
    let color = ProjectColor::for_project_name(&item.project_name);

    view()
        .p(px(16.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .flex_row()
        .items_start()
        .justify_between()
        .gap(px(16.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(6.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            view()
                                .px(px(8.0))
                                .py(px(2.0))
                                .rounded_full()
                                .bg(palette::subtle_surface_bg())
                                .child(
                                    text(item.kind.label())
                                        .text_size(px(11.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_secondary()),
                                ),
                        )
                        .child(project_dot(color))
                        .child(
                            text(format!("{} · {}", item.project_name, item.run_label))
                                .text_size(px(12.0))
                                .text_color(palette::text_muted()),
                        )
                        .child(
                            text(format!(
                                "Waiting {}",
                                format_wait_minutes(item.waiting_minutes)
                            ))
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    text(item.title.clone())
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(item.detail.clone())
                        .text_size(px(12.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(if resolved {
            view()
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(7.0))
                .bg(palette::subtle_surface_bg())
                .child(
                    text("Answered")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::success_text()),
                )
                .into_element()
        } else {
            button()
                .on_press(move || store::resolve_inbox_item(id))
                .px(px(12.0))
                .py(px(6.0))
                .rounded(px(7.0))
                .bg(palette::nav_active_bg())
                .active(|s| s.opacity(0.8))
                .child(
                    text("Approve")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                )
                .into_element()
        })
}
