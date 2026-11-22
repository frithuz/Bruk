//! Native macOS menu bar item (`NSStatusItem`), Dock badge sync, notification
//! listener, and floating "Waiting on you" popover window (`10 AttentionSurfaces.png`).

use factory_core::AttentionRequest;
use nsh_base::button;
use nsh_core::{
    desktop::{
        MenuItem as DesktopMenuItem, MenuRole, TrayTitlePlacement, TrayTitleRun, WindowAttributes,
        WindowFrame, WindowFrameStyle, WindowKind, WindowLevel, WindowPosition, Workspaces,
    },
    element::{
        ImageSrc,
        desktop::{MenuItem, current_window, displays, main_window, quit, set_tray, tray, window},
        next_notification_press, set_badge,
    },
    geometry::{Bounds, Point, Points, Size},
    prelude::*,
};

use crate::{store, ui::palette};

/// Double-pine template icon rendered in the macOS menu bar (`NSStatusBarButton`).
///
/// Using black strokes/fills in an SVG lets AppKit treat the image as a template
/// icon that adapts automatically to light and dark menu bars.
const TRAY_TREES_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 18" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M6.4 2.2 2.4 12.6h8L6.4 2.2Z"/><path d="M6.4 12.6v3.2"/><path d="M13.6 2.8 9.9 12.6h7.4L13.6 2.8Z"/><path d="M13.6 12.6v3.2"/></svg>"#;

const POPOVER_WIDTH: f32 = 368.0;
const POPOVER_HEADER_HEIGHT: f32 = 42.0;
const POPOVER_ROW_HEIGHT: f32 = 58.0;
const POPOVER_EMPTY_HEIGHT: f32 = 62.0;
const POPOVER_FOOTER_HEIGHT: f32 = 108.0;
const POPOVER_MAX_ITEMS: usize = 6;

/// Reactive snapshot driving the native macOS menu bar item and Dock badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TraySnapshot {
    pub(crate) pending_count: usize,
    pub(crate) any_overdue: bool,
    pub(crate) active_agents: u32,
    pub(crate) all_agents_paused: bool,
    pub(crate) snoozed_minutes: Option<u32>,
}

impl TraySnapshot {
    pub(crate) fn current() -> Self {
        let inbox = store::inbox_state();
        Self {
            pending_count: inbox.pending_count(),
            any_overdue: inbox.any_overdue(),
            active_agents: inbox.active_agents_count,
            all_agents_paused: inbox.all_agents_paused,
            snoozed_minutes: inbox.notifications_snoozed_minutes,
        }
    }

    pub(crate) fn tooltip(&self) -> String {
        if self.pending_count == 0 {
            if self.all_agents_paused {
                "Bruk — All caught up · Agents paused".to_owned()
            } else if self.active_agents == 0 {
                "Bruk — All caught up · Idle".to_owned()
            } else {
                format!(
                    "Bruk — All caught up · {} agents working",
                    self.active_agents
                )
            }
        } else {
            let status = if self.all_agents_paused {
                "Agents paused".to_owned()
            } else {
                format!("{} agents working", self.active_agents)
            };
            format!("Bruk — {} waiting on you · {status}", self.pending_count)
        }
    }

    pub(crate) fn title_runs(&self) -> Vec<TrayTitleRun> {
        if self.pending_count == 0 {
            return Vec::new();
        }
        let run = TrayTitleRun::new(format!(" {}", self.pending_count));
        if self.any_overdue {
            vec![run.color(rgb(0xDC5B24))]
        } else {
            vec![run]
        }
    }
}

/// Installs the reactive macOS menu bar status item, Dock badge updater, and
/// notification click handler for the application root.
pub(crate) fn install_menubar_and_attention_surfaces() {
    let snapshot = computed(TraySnapshot::current);
    effect(move || {
        let snap = snapshot.get();
        set_badge(snap.pending_count as u32);
        set_tray(build_tray(&snap));
    });

    // Clicking any OS notification opens the separate Inbox window directly on that request.
    spawn(async move {
        loop {
            let pressed_id = next_notification_press().await;
            if !pressed_id.is_empty() {
                store::open_inbox_for_request(&pressed_id);
            }
        }
    });
}

fn build_tray(snap: &TraySnapshot) -> nsh_core::element::desktop::Tray {
    let pause_label = if snap.all_agents_paused {
        "Resume All Agents"
    } else {
        "Pause All Agents"
    };
    let snooze_label = if snap.snoozed_minutes.is_some() {
        "Resume Notifications"
    } else {
        "Snooze Notifications for 1 Hour"
    };

    tray()
        .icon(ImageSrc::svg_markup(TRAY_TREES_SVG))
        .tooltip(snap.tooltip())
        .title(snap.title_runs())
        .title_placement(TrayTitlePlacement::Right)
        .menu([
            MenuItem::new("Open Inbox")
                .shortcut("mod+i")
                .on_select(store::open_inbox),
            MenuItem::new(pause_label).on_select(store::toggle_pause_all_agents),
            MenuItem::new(snooze_label).on_select(store::toggle_snooze_notifications),
            MenuItem::separator(),
            MenuItem::new("Show Bruk").on_select(|| main_window().focus()),
            MenuItem::separator(),
            MenuItem::new("Quit Bruk").role_like_quit().on_select(quit),
        ])
        .on_click(|event| store::toggle_menubar_popover(event.bounds))
}

trait MenuItemExt {
    fn role_like_quit(self) -> Self;
}

impl MenuItemExt for MenuItem {
    fn role_like_quit(self) -> Self {
        let _ = (DesktopMenuItem::Separator, MenuRole::Quit);
        self.shortcut("mod+q")
    }
}

fn popover_height_for_count(pending_count: usize) -> f32 {
    let body = if pending_count == 0 {
        POPOVER_EMPTY_HEIGHT
    } else {
        let visible = pending_count.min(POPOVER_MAX_ITEMS) as f32;
        visible * POPOVER_ROW_HEIGHT + 8.0
    };
    POPOVER_HEADER_HEIGHT + body + POPOVER_FOOTER_HEIGHT
}

fn compute_popover_origin(anchor: Option<Bounds<Points>>, height: f32) -> Point<Points> {
    let screen = displays();
    let visible = screen
        .iter()
        .find(|d| d.primary)
        .or_else(|| screen.first())
        .map(|d| d.visible_bounds);

    let margin = 8.0_f32;
    if let Some(anchor) = anchor {
        let desired_x = anchor.origin.x.0 + anchor.size.width.0 - POPOVER_WIDTH;
        let desired_y = anchor.origin.y.0 + anchor.size.height.0 + 6.0;
        if let Some(bounds) = visible {
            let min_x = bounds.origin.x.0 + margin;
            let max_x =
                (bounds.origin.x.0 + bounds.size.width.0 - POPOVER_WIDTH - margin).max(min_x);
            let min_y = bounds.origin.y.0 + margin;
            let max_y = (bounds.origin.y.0 + bounds.size.height.0 - height - margin).max(min_y);
            return Point::new(
                px(desired_x.clamp(min_x, max_x)),
                px(desired_y.clamp(min_y, max_y)),
            );
        }
        return Point::new(px(desired_x.max(margin)), px(desired_y.max(margin)));
    }

    if let Some(bounds) = visible {
        let x = (bounds.origin.x.0 + bounds.size.width.0 - POPOVER_WIDTH - 14.0)
            .max(bounds.origin.x.0 + margin);
        let y = bounds.origin.y.0 + 8.0;
        Point::new(px(x), px(y))
    } else {
        Point::new(px(960.0), px(34.0))
    }
}

/// Declarative floating popover window anchored beneath the macOS menu bar status item.
pub(crate) fn menubar_popover_window() -> impl IntoElement {
    let mut attrs = WindowAttributes::default();
    attrs.frame = Some(WindowFrameStyle::None);
    attrs.resizable = Some(false);
    attrs.minimizable = Some(false);
    attrs.maximizable = Some(false);
    attrs.close_on_escape = Some(true);
    attrs.close_on_blur = Some(true);
    attrs.level = Some(WindowLevel::Floating);
    attrs.taskbar = Some(false);
    attrs.workspaces = Some(Workspaces::All);
    attrs.transparent = Some(true);
    attrs.shadow = Some(true);

    let initial_height = popover_height_for_count(store::pending_inbox_count());
    let initial_origin = compute_popover_origin(store::menubar_anchor_bounds(), initial_height);

    window()
        .id("menubar-popover")
        .kind(WindowKind::Popup)
        .default_size(Size::new(px(POPOVER_WIDTH), px(initial_height)))
        .default_position(WindowPosition::At(initial_origin))
        .title("Waiting on you")
        .attributes(attrs)
        .open(store::menubar_popover_open)
        .on_open_change(|change| {
            if !change.open {
                store::close_menubar_popover();
            }
        })
        .content(menubar_popover_surface)
}

fn menubar_popover_surface() -> impl IntoElement {
    let win = current_window();
    effect(move || {
        let count = store::pending_inbox_count();
        let anchor = store::menubar_anchor_bounds();
        let height = popover_height_for_count(count);
        let origin = compute_popover_origin(anchor, height);
        let frame = WindowFrame {
            x: Some(origin.x),
            y: Some(origin.y),
            width: Some(px(POPOVER_WIDTH)),
            height: Some(px(height)),
            ..Default::default()
        };
        win.set_bounds(frame);
    });

    view()
        .flex_1()
        .bg(palette::popover_bg())
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::popover_border())
        .overflow_hidden()
        .child(popover_header())
        .child(popover_waiting_list())
        .child(popover_footer_actions())
}

fn popover_header() -> impl IntoElement {
    view()
        .h(px(POPOVER_HEADER_HEIGHT))
        .flex_shrink_0()
        .px(px(14.0))
        .flex_row()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(palette::divider_color())
        .child(
            text(move || {
                let count = store::pending_inbox_count();
                if count == 0 {
                    "All caught up".to_owned()
                } else {
                    format!("Waiting on you · {count}")
                }
            })
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_primary()),
        )
        .child(
            text(move || store::inbox_state().menubar_agents_status())
                .text_size(px(11.5))
                .text_color(move || {
                    if store::inbox_state().all_agents_paused {
                        palette::warning_text()
                    } else {
                        palette::text_muted()
                    }
                }),
        )
}

fn popover_waiting_list() -> impl IntoElement {
    scroll().flex_1().min_h(px(0.0)).child(dynamic(|| {
        let inbox = store::inbox_state();
        let items: Vec<AttentionRequest> =
            inbox.pending_newest_first().into_iter().cloned().collect();

        if items.is_empty() {
            return view()
                .px(px(14.0))
                .py(px(18.0))
                .gap(px(2.0))
                .child(
                    text("No agents are waiting on you")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("New permissions, questions, and review gates will appear here.")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .into_element();
        }

        let mut list = view().py(px(4.0));
        for req in items {
            list = list.child(popover_request_row(&req));
        }
        list.into_element()
    }))
}

fn popover_request_row(req: &AttentionRequest) -> impl IntoElement {
    let req_id = req.id.clone();
    let open_id = req.id.clone();
    let dot_color = if req.past_reminder_threshold {
        palette::menubar_overdue_dot()
    } else {
        palette::menubar_fresh_dot()
    };

    let trailing: AnyElement = if req.allows_menubar_quick_answer() {
        let allow_id = req.id.clone();
        let deny_id = req.id.clone();
        let allow_test_id = format!("menubar-allow-{}", req.id);
        let deny_test_id = format!("menubar-deny-{}", req.id);
        view()
            .flex_row()
            .items_center()
            .gap(px(5.0))
            .flex_shrink_0()
            .child(
                button()
                    .test_id(&allow_test_id)
                    .accessibility_label(format!("Allow {}", req.menubar_title))
                    .on_press(move || store::inbox_quick_allow(&allow_id))
                    .h(px(24.0))
                    .px(px(9.5))
                    .rounded(px(6.0))
                    .bg(palette::nav_active_bg())
                    .items_center()
                    .justify_center()
                    .active(|s| s.opacity(0.82))
                    .child(
                        text("Allow")
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::nav_active_fg()),
                    ),
            )
            .child(
                button()
                    .test_id(&deny_test_id)
                    .accessibility_label(format!("Deny {}", req.menubar_title))
                    .on_press(move || store::inbox_quick_deny(&deny_id))
                    .h(px(24.0))
                    .px(px(8.5))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::control_bg())
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                    .active(|s| s.opacity(0.8))
                    .child(
                        text("Deny")
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::destructive_text()),
                    ),
            )
            .into_element()
    } else {
        let action_label = req.menubar_action_label();
        let action_test_id = format!("menubar-action-{}", req.id);
        let action_id = req.id.clone();
        button()
            .test_id(&action_test_id)
            .accessibility_label(format!("{action_label} {}", req.menubar_title))
            .on_press(move || store::open_inbox_for_request(&action_id))
            .h(px(24.0))
            .px(px(10.0))
            .flex_shrink_0()
            .rounded(px(6.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::control_bg())
            .items_center()
            .justify_center()
            .hover(|s| s.bg(palette::subtle_surface_bg()))
            .active(|s| s.opacity(0.8))
            .child(
                text(action_label)
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_primary()),
            )
            .into_element()
    };

    let row_test_id = format!("menubar-row-{req_id}");
    view()
        .h(px(POPOVER_ROW_HEIGHT))
        .px(px(12.0))
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            button()
                .test_id(&row_test_id)
                .accessibility_label(format!("Open {} in Inbox", req.menubar_title))
                .on_press(move || store::open_inbox_for_request(&open_id))
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .py(px(6.0))
                .child(
                    view()
                        .w(px(7.5))
                        .h(px(7.5))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(dot_color),
                )
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(2.0))
                        .child(
                            text(req.menubar_title.clone())
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary())
                                .truncate(),
                        )
                        .child(
                            text(req.menubar_meta_line())
                                .text_size(px(11.0))
                                .text_color(palette::text_muted())
                                .truncate(),
                        ),
                ),
        )
        .child(trailing)
}

fn popover_footer_actions() -> impl IntoElement {
    view()
        .flex_shrink_0()
        .border_t_1()
        .border_color(palette::divider_color())
        .py(px(6.0))
        .px(px(6.0))
        .gap(px(1.0))
        .child(
            button()
                .test_id("menubar-open-inbox")
                .accessibility_label("Open Inbox")
                .on_press(store::open_inbox)
                .w_full()
                .h(px(30.0))
                .px(px(10.0))
                .rounded(px(6.0))
                .flex_row()
                .items_center()
                .justify_between()
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Open Inbox")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("⌘I")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            button()
                .test_id("menubar-pause-agents")
                .accessibility_label("Pause All Agents")
                .on_press(store::toggle_pause_all_agents)
                .w_full()
                .h(px(30.0))
                .px(px(10.0))
                .rounded(px(6.0))
                .flex_row()
                .items_center()
                .justify_between()
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.8))
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
                    .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .test_id("menubar-snooze-notifications")
                .accessibility_label("Snooze Notifications for 1 Hour")
                .on_press(store::toggle_snooze_notifications)
                .w_full()
                .h(px(30.0))
                .px(px(10.0))
                .rounded(px(6.0))
                .flex_row()
                .items_center()
                .justify_between()
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text(move || {
                        if let Some(mins) = store::inbox_state().notifications_snoozed_minutes {
                            format!("Snoozed ({mins} min) — Resume")
                        } else {
                            "Snooze Notifications for 1 Hour".to_owned()
                        }
                    })
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(move || {
                        if store::inbox_state().notifications_snoozed_minutes.is_some() {
                            palette::warning_text()
                        } else {
                            palette::text_primary()
                        }
                    }),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_snapshot_turns_orange_when_overdue_and_clears_when_empty() {
        let overdue = TraySnapshot {
            pending_count: 5,
            any_overdue: true,
            active_agents: 3,
            all_agents_paused: false,
            snoozed_minutes: None,
        };
        let runs = overdue.title_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, " 5");
        assert!(runs[0].color.is_some());
        assert!(overdue.tooltip().contains("5 waiting on you"));

        let fresh = TraySnapshot {
            pending_count: 1,
            any_overdue: false,
            ..overdue.clone()
        };
        let fresh_runs = fresh.title_runs();
        assert_eq!(fresh_runs.len(), 1);
        assert_eq!(fresh_runs[0].text, " 1");
        assert!(fresh_runs[0].color.is_none());

        let empty = TraySnapshot {
            pending_count: 0,
            any_overdue: false,
            ..overdue
        };
        assert!(empty.title_runs().is_empty());
        assert!(empty.tooltip().contains("All caught up"));
    }

    #[test]
    fn popover_height_scales_with_pending_count() {
        assert_eq!(
            popover_height_for_count(0),
            POPOVER_HEADER_HEIGHT + POPOVER_EMPTY_HEIGHT + POPOVER_FOOTER_HEIGHT
        );
        assert!(popover_height_for_count(5) > popover_height_for_count(2));
        assert_eq!(popover_height_for_count(10), popover_height_for_count(6));
    }
}
