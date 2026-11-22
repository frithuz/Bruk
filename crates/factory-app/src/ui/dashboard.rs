//! Dashboard landing page: KPI overview, daily token usage chart/table, project & provider
//! breakdowns, project activity table, highest token runs, wait latency, and insights.

use factory_core::{
    DashboardInsight, DashboardSnapshot, HighTokenRun, PreferencesTab, ProjectActivityRow,
    ProjectColor, ProjectTokens, ProviderUsage, TokenBucket, format_axis_tokens, format_currency,
    format_tenths_hours, format_tokens, format_wait_minutes,
};
use nsh_base::button;
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{icons, palette},
};

const RIGHT_COLUMN_WIDTH: f32 = 344.0;
const CHART_PLOT_HEIGHT: f32 = 196.0;

/// Main scrollable Dashboard canvas matching the reference overview layout.
pub(crate) fn dashboard_view() -> impl IntoElement {
    scroll()
        .flex_1()
        .bg(palette::canvas_bg())
        .child(dynamic(|| {
            let snap = store::dashboard_snapshot();
            let show_table = store::tokens_show_table();

            if snap.is_empty() && store::project_ids().is_empty() {
                return dashboard_empty_state().into_element();
            }

            view()
                .flex_1()
                .p(px(18.0))
                .gap(px(14.0))
                .child(kpi_row(&snap))
                .child(
                    view()
                        .flex_row()
                        .items_stretch()
                        .gap(px(14.0))
                        .child(tokens_per_day_card(&snap, show_table))
                        .child(tokens_by_project_card(&snap)),
                )
                .child(
                    view()
                        .flex_row()
                        .items_start()
                        .gap(px(14.0))
                        .child(projects_worked_on_card(&snap))
                        .child(highest_token_runs_card(&snap)),
                )
                .child(
                    view()
                        .flex_row()
                        .items_stretch()
                        .gap(px(14.0))
                        .child(wait_time_card(&snap))
                        .child(worth_a_look_card(&snap)),
                )
                .into_element()
        }))
}

fn dashboard_empty_state() -> impl IntoElement {
    view()
        .test_id("dashboard-empty-state")
        .flex_1()
        .items_center()
        .justify_center()
        .px(px(28.0))
        .py(px(64.0))
        .child(
            card()
                .w(px(520.0))
                .p(px(28.0))
                .items_center()
                .gap(px(14.0))
                .child(
                    view()
                        .w(px(44.0))
                        .h(px(44.0))
                        .rounded(px(11.0))
                        .bg(palette::subtle_surface_bg())
                        .border_1()
                        .border_color(palette::divider_color())
                        .items_center()
                        .justify_center()
                        .child(icons::dashboard(palette::text_secondary())),
                )
                .child(
                    view()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text("Welcome to Bruk")
                                .text_size(px(18.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(
                                "Register a local repository, connect an AI provider, and run workflows to see activity, token usage, and latency metrics here.",
                            )
                            .text_center()
                            .text_size(px(13.0))
                            .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    view()
                        .pt(px(4.0))
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            button()
                                .test_id("dashboard-empty-register-project")
                                .accessibility_label("Register a Project")
                                .on_press(store::open_register_project)
                                .h(px(34.0))
                                .px(px(15.0))
                                .rounded(px(8.0))
                                .bg(palette::nav_active_bg())
                                .items_center()
                                .justify_center()
                                .active(|s| s.opacity(0.85))
                                .child(
                                    text("Register a Project")
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::nav_active_fg()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("dashboard-empty-providers")
                                .accessibility_label("Configure Providers")
                                .on_press(|| store::open_preferences_tab(PreferencesTab::Providers))
                                .h(px(34.0))
                                .px(px(14.0))
                                .rounded(px(8.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text("Configure Providers…")
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("dashboard-empty-machines")
                                .accessibility_label("Add a Machine")
                                .on_press(store::open_machines)
                                .h(px(34.0))
                                .px(px(14.0))
                                .rounded(px(8.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text("Machines")
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        ),
                )
                .child(
                    view()
                        .w_full()
                        .pt(px(10.0))
                        .mt(px(4.0))
                        .border_t_1()
                        .border_color(palette::divider_color())
                        .items_center()
                        .child(
                            text(
                                "Want to preview visuals with sample data? Enable Demo Mode in the Bruk menu.",
                            )
                            .text_center()
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                        ),
                ),
        )
}

fn card() -> View {
    view()
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
}

// --- Row 1: 6 KPI Cards -----------------------------------------------------------------

fn kpi_row(snap: &DashboardSnapshot) -> impl IntoElement {
    let runs_sub = format!(
        "{} succeeded · {} failed",
        snap.succeeded_runs, snap.failed_runs
    );
    let cost_sub = format!(
        "At API rates · {} billed",
        format_currency(snap.billed_cost_cents)
    );

    view()
        .flex_row()
        .items_stretch()
        .gap(px(12.0))
        .child(kpi_card(
            "Issues resolved",
            snap.issues_resolved.to_string(),
            snap.issues_delta_text.clone(),
        ))
        .child(kpi_card(
            "Pull requests merged",
            snap.prs_merged.to_string(),
            snap.prs_delta_text.clone(),
        ))
        .child(kpi_card("Runs", snap.total_runs.to_string(), runs_sub))
        .child(kpi_card(
            "Tokens used",
            format_tokens(snap.total_tokens),
            snap.tokens_delta_text.clone(),
        ))
        .child(kpi_card(
            "Estimated cost",
            format_currency(snap.est_cost_cents),
            cost_sub,
        ))
        .child(kpi_card(
            "Agent working time",
            format_tenths_hours(snap.working_time_tenths_hours),
            "Across all runs".to_owned(),
        ))
}

fn kpi_card(label: &'static str, value: String, subtitle: String) -> impl IntoElement {
    card()
        .flex_1()
        .min_w(px(0.0))
        .px(px(14.0))
        .py(px(13.0))
        .gap(px(5.0))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::NORMAL)
                .text_color(palette::text_muted()),
        )
        .child(
            text(value)
                .text_size(px(26.0))
                .font_weight(FontWeight::BOLD)
                .tabular_nums()
                .text_color(palette::text_primary()),
        )
        .child(
            text(subtitle)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}

// --- Row 2 Left: Tokens per day ---------------------------------------------------------

fn tokens_per_day_card(snap: &DashboardSnapshot, show_table: bool) -> impl IntoElement {
    let mut legend = view().flex_row().items_center().gap(px(12.0));
    for project in &snap.project_tokens {
        legend = legend.child(legend_item(&project.project_name, project.color));
    }

    let has_buckets = !snap.buckets.is_empty();
    let toggle_label = if show_table {
        "Show chart"
    } else {
        "Show table"
    };

    let mut header_right = view().flex_row().items_center().gap(px(14.0)).child(legend);
    if has_buckets {
        header_right = header_right.child(
            button()
                .on_press(store::toggle_tokens_show_table)
                .px(px(10.0))
                .py(px(4.5))
                .rounded(px(7.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text(toggle_label)
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    card()
        .flex_1()
        .min_w(px(0.0))
        .p(px(16.0))
        .gap(px(14.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    text("Tokens per day")
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(header_right),
        )
        .child(if !has_buckets {
            view()
                .h(px(CHART_PLOT_HEIGHT))
                .items_center()
                .justify_center()
                .gap(px(4.0))
                .child(
                    text("No token usage recorded yet")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                )
                .child(
                    text("Daily token consumption by project will appear here once runs execute.")
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                )
                .into_element()
        } else if show_table {
            tokens_table_view(snap).into_element()
        } else {
            tokens_chart_view(snap).into_element()
        })
}

fn legend_item(name: &str, color: ProjectColor) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(project_dot(color))
        .child(
            text(name.to_owned())
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}

fn tokens_chart_view(snap: &DashboardSnapshot) -> impl IntoElement {
    let max_tokens = snap.chart_max_tokens.max(1);
    let mid_tokens = max_tokens / 2;
    let bar_width = if snap.buckets.len() <= 4 { 52.0 } else { 36.0 };

    let mut bars_row = view()
        .absolute()
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .left(px(0.0))
        .flex_row()
        .items_end()
        .px(px(12.0));

    let mut x_axis_row = view().flex_row().flex_1().px(px(12.0));

    for bucket in &snap.buckets {
        bars_row = bars_row.child(stacked_bar_column(bucket, max_tokens, bar_width));
        x_axis_row = x_axis_row.child(
            view().flex_1().items_center().child(
                text(bucket.label.clone())
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
            ),
        );
    }

    view()
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_stretch()
                .child(
                    view()
                        .w(px(36.0))
                        .h(px(CHART_PLOT_HEIGHT))
                        .pr(px(10.0))
                        .justify_between()
                        .items_end()
                        .child(axis_tick(format_axis_tokens(max_tokens)))
                        .child(axis_tick(format_axis_tokens(mid_tokens)))
                        .child(axis_tick("0".to_owned())),
                )
                .child(
                    view()
                        .flex_1()
                        .h(px(CHART_PLOT_HEIGHT))
                        .relative()
                        .child(
                            view()
                                .absolute()
                                .top(px(0.0))
                                .right(px(0.0))
                                .bottom(px(0.0))
                                .left(px(0.0))
                                .justify_between()
                                .child(gridline())
                                .child(gridline())
                                .child(gridline()),
                        )
                        .child(bars_row),
                ),
        )
        .child(view().flex_row().pl(px(36.0)).child(x_axis_row))
}

fn axis_tick(label: String) -> impl IntoElement {
    text(label)
        .text_size(px(11.0))
        .tabular_nums()
        .text_color(palette::text_muted())
}

fn gridline() -> impl IntoElement {
    view().w_full().h(px(1.0)).bg(palette::gridline_color())
}

fn stacked_bar_column(bucket: &TokenBucket, max_tokens: u64, bar_width: f32) -> impl IntoElement {
    // Non-zero segments stacked bottom-to-top (first project at bottom, last at top).
    let non_zero: Vec<&ProjectTokens> = bucket.segments.iter().filter(|s| s.tokens > 0).collect();

    let mut stack = view()
        .w(px(bar_width))
        .gap(px(2.0))
        .items_stretch()
        .justify_end();

    // Render from top segment down to bottom segment.
    for (idx, seg) in non_zero.iter().rev().enumerate() {
        let is_top = idx == 0;
        let is_bottom = idx + 1 == non_zero.len();
        let raw_height = (seg.tokens as f32 / max_tokens as f32) * (CHART_PLOT_HEIGHT - 2.0);
        let seg_height = raw_height.max(3.5);

        let mut bar_seg = view()
            .w_full()
            .h(px(seg_height))
            .bg(palette::project_color(seg.color));

        if is_top && is_bottom {
            bar_seg = bar_seg.rounded_t(px(4.0)).rounded_b(px(1.0));
        } else if is_top {
            bar_seg = bar_seg.rounded_t(px(4.0)).rounded_b(px(1.5));
        } else if is_bottom {
            bar_seg = bar_seg.rounded_t(px(1.5));
        } else {
            bar_seg = bar_seg.rounded(px(1.5));
        }

        stack = stack.child(bar_seg);
    }

    view()
        .flex_1()
        .h_full()
        .items_center()
        .justify_end()
        .child(stack)
}

fn tokens_table_view(snap: &DashboardSnapshot) -> impl IntoElement {
    let project_names: Vec<String> = snap
        .project_tokens
        .iter()
        .map(|p| p.project_name.clone())
        .collect();

    let mut header = view()
        .flex_row()
        .items_center()
        .pb(px(8.0))
        .border_b_1()
        .border_color(palette::divider_color())
        .child(
            text("Period")
                .w(px(64.0))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        );

    for name in &project_names {
        header = header.child(
            text(name.clone())
                .flex_1()
                .text_right()
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_muted()),
        );
    }

    header = header.child(
        text("Total")
            .w(px(76.0))
            .text_right()
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::text_muted()),
    );

    let mut rows = view();
    for (idx, bucket) in snap.buckets.iter().enumerate() {
        let mut row = view().flex_row().items_center().py(px(7.0)).child(
            text(bucket.label.clone())
                .w(px(64.0))
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        );

        if idx + 1 < snap.buckets.len() {
            row = row.border_b_1().border_color(palette::divider_color());
        }

        for name in &project_names {
            let value = bucket
                .segments
                .iter()
                .find(|s| &s.project_name == name)
                .map(|s| s.tokens)
                .unwrap_or(0);
            row = row.child(
                text(format_tokens(value))
                    .flex_1()
                    .text_right()
                    .tabular_nums()
                    .text_size(px(12.5))
                    .text_color(palette::text_secondary()),
            );
        }

        row = row.child(
            text(format_tokens(bucket.total_tokens))
                .w(px(76.0))
                .text_right()
                .tabular_nums()
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        );

        rows = rows.child(row);
    }

    view().child(header).child(rows)
}

// --- Row 2 Right: Tokens by project -----------------------------------------------------

fn tokens_by_project_card(snap: &DashboardSnapshot) -> impl IntoElement {
    let max_project = snap
        .project_tokens
        .iter()
        .map(|p| p.tokens)
        .max()
        .unwrap_or(1)
        .max(1);

    let max_provider = snap
        .providers
        .iter()
        .map(|p| p.tokens)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut project_bars = view().gap(px(12.0));
    if snap.project_tokens.is_empty() {
        project_bars = project_bars.child(
            text("No project token usage yet.")
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    } else {
        for item in &snap.project_tokens {
            project_bars = project_bars.child(project_token_bar_row(item, max_project));
        }
    }

    let mut provider_bars = view().gap(px(8.0));
    if snap.providers.is_empty() {
        provider_bars = provider_bars.child(
            text("No provider usage recorded yet.")
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    } else {
        for provider in &snap.providers {
            provider_bars = provider_bars.child(provider_usage_row(provider, max_provider));
        }
    }

    card()
        .w(px(RIGHT_COLUMN_WIDTH))
        .flex_shrink_0()
        .p(px(16.0))
        .gap(px(14.0))
        .child(
            text("Tokens by project")
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(project_bars)
        .child(view().w_full().h(px(1.0)).bg(palette::divider_color()))
        .child(
            view()
                .gap(px(9.0))
                .child(
                    text("By provider")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_muted()),
                )
                .child(provider_bars),
        )
        .child(
            text(snap.provider_footnote.clone())
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
        )
}

fn project_token_bar_row(item: &ProjectTokens, max_tokens: u64) -> impl IntoElement {
    let fraction = if item.tokens == 0 {
        0.0
    } else {
        (item.tokens as f32 / max_tokens as f32).clamp(0.05, 1.0)
    };

    view()
        .gap(px(5.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(item.project_name.clone())
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(format_tokens(item.tokens))
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .tabular_nums()
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(format!("{}%", item.share_pct))
                                .w(px(30.0))
                                .text_right()
                                .text_size(px(12.0))
                                .tabular_nums()
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(
            view().w_full().h(px(8.0)).flex_row().child(
                view()
                    .h_full()
                    .w(relative(fraction))
                    .rounded_full()
                    .bg(palette::project_color(item.color)),
            ),
        )
}

fn provider_usage_row(provider: &ProviderUsage, max_tokens: u64) -> impl IntoElement {
    let fraction = if provider.tokens == 0 {
        0.0
    } else {
        (provider.tokens as f32 / max_tokens as f32).clamp(0.05, 1.0)
    };

    view()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .child(
            text(provider.provider_name.clone())
                .w(px(74.0))
                .text_size(px(12.0))
                .text_color(palette::text_secondary()),
        )
        .child(
            view().flex_1().h(px(7.0)).flex_row().child(
                view()
                    .h_full()
                    .w(relative(fraction * 0.88))
                    .rounded_full()
                    .bg(palette::provider_bar_fill()),
            ),
        )
        .child(
            text(format_tokens(provider.tokens))
                .w(px(40.0))
                .text_right()
                .tabular_nums()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
}

// --- Row 3 Left: Projects worked on -----------------------------------------------------

fn projects_worked_on_card(snap: &DashboardSnapshot) -> impl IntoElement {
    let mut rows = view();
    if snap.project_rows.is_empty() {
        rows = rows.child(
            view().py(px(18.0)).items_center().child(
                text("No projects worked on yet.")
                    .text_size(px(12.5))
                    .text_color(palette::text_muted()),
            ),
        );
    } else {
        for (idx, row) in snap.project_rows.iter().enumerate() {
            let is_last = idx + 1 == snap.project_rows.len();
            rows = rows.child(project_activity_row(row, is_last));
        }
    }

    card()
        .flex_1()
        .min_w(px(0.0))
        .p(px(16.0))
        .gap(px(10.0))
        .child(
            text("Projects worked on")
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .pb(px(8.0))
                .border_b_1()
                .border_color(palette::divider_color())
                .child(table_col_header("Project").flex_1())
                .child(
                    table_col_header("Issues resolved")
                        .w(px(100.0))
                        .text_right(),
                )
                .child(table_col_header("PRs merged").w(px(84.0)).text_right())
                .child(table_col_header("Runs").w(px(56.0)).text_right())
                .child(table_col_header("Succeeded").w(px(92.0)).text_right())
                .child(table_col_header("Est. cost").w(px(76.0)).text_right()),
        )
        .child(rows)
}

fn table_col_header(label: &'static str) -> Text {
    text(label)
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::text_muted())
}

fn project_activity_row(row: &ProjectActivityRow, is_last: bool) -> impl IntoElement {
    let project_name = row.project_name.clone();
    let toggle_filter = move || {
        let current = store::dashboard_project_filter();
        if current.as_deref() == Some(project_name.as_str()) {
            store::set_dashboard_project_filter(None);
        } else {
            store::set_dashboard_project_filter(Some(project_name.clone()));
        }
    };

    let succeeded_text = if row.runs == 0 {
        "0 · 0%".to_owned()
    } else {
        format!("{} · {}%", row.succeeded, row.success_rate_pct())
    };

    let mut container = button()
        .accessibility_label(format!("Filter by {}", row.project_name))
        .on_press(toggle_filter)
        .flex_row()
        .items_center()
        .py(px(10.0))
        .px(px(4.0))
        .rounded(px(6.0))
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(
            view()
                .flex_1()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(project_dot(row.color))
                .child(
                    text(row.project_name.clone())
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(format!("· {}", row.source))
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            text(row.issues_resolved.to_string())
                .w(px(100.0))
                .text_right()
                .tabular_nums()
                .text_size(px(12.5))
                .text_color(palette::text_primary()),
        )
        .child(
            text(row.prs_merged.to_string())
                .w(px(84.0))
                .text_right()
                .tabular_nums()
                .text_size(px(12.5))
                .text_color(palette::text_primary()),
        )
        .child(
            text(row.runs.to_string())
                .w(px(56.0))
                .text_right()
                .tabular_nums()
                .text_size(px(12.5))
                .text_color(palette::text_primary()),
        )
        .child(
            text(succeeded_text)
                .w(px(92.0))
                .text_right()
                .tabular_nums()
                .text_size(px(12.5))
                .text_color(palette::text_primary()),
        )
        .child(
            text(format_currency(row.est_cost_cents))
                .w(px(76.0))
                .text_right()
                .tabular_nums()
                .text_size(px(12.5))
                .text_color(palette::text_primary()),
        );

    if !is_last {
        container = container
            .border_b_1()
            .border_color(palette::divider_color());
    }

    container
}

// --- Row 3 Right: Highest token runs ----------------------------------------------------

fn highest_token_runs_card(snap: &DashboardSnapshot) -> impl IntoElement {
    let mut list = view().gap(px(9.0));
    if snap.highest_token_runs.is_empty() {
        list = list.child(
            text("No high-token runs recorded for this selection.")
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    } else {
        for run in &snap.highest_token_runs {
            list = list.child(high_token_run_item(run));
        }
    }

    card()
        .w(px(RIGHT_COLUMN_WIDTH))
        .flex_shrink_0()
        .p(px(16.0))
        .gap(px(12.0))
        .child(
            text("Highest token runs")
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(list)
}

fn high_token_run_item(run: &HighTokenRun) -> impl IntoElement {
    view()
        .flex_row()
        .items_start()
        .gap(px(10.0))
        .p(px(11.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .child(
            view()
                .w(px(20.0))
                .h(px(20.0))
                .mt(px(1.0))
                .rounded_full()
                .bg(palette::rank_badge_bg())
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .child(
                    text(run.rank.to_string())
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(3.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.0))
                        .child(
                            text(run.title.clone())
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(format_tokens(run.tokens))
                                .text_size(px(12.5))
                                .font_weight(FontWeight::BOLD)
                                .tabular_nums()
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    text(format!("{} · {}", run.project_name, run.subtitle))
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(format!(
                        "{} · {} min · {}",
                        format_currency(run.est_cost_cents),
                        run.duration_minutes,
                        run.outcome_note
                    ))
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                ),
        )
}

// --- Row 4 Left: Time agents waited on you ----------------------------------------------

fn wait_time_card(snap: &DashboardSnapshot) -> impl IntoElement {
    let wait = &snap.wait_metrics;
    let median_display = if wait.requests_answered == 0 {
        "—".to_owned()
    } else {
        format_wait_minutes(wait.median_answer_minutes)
    };
    let longest_display = if wait.requests_answered == 0 {
        "—".to_owned()
    } else {
        format_wait_minutes(wait.longest_wait_minutes)
    };

    card()
        .flex_grow(1.18)
        .min_w(px(0.0))
        .p(px(16.0))
        .gap(px(14.0))
        .child(
            text("Time agents waited on you")
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_start()
                .gap(px(32.0))
                .child(wait_stat(median_display, "Median answer time"))
                .child(wait_stat(longest_display, "Longest wait"))
                .child(wait_stat(
                    format_tenths_hours(wait.total_waiting_tenths_hours),
                    "Agent time spent waiting",
                )),
        )
        .child(
            text(wait.summary_sentence())
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
}

fn wait_stat(value: String, label: &'static str) -> impl IntoElement {
    view()
        .gap(px(3.0))
        .child(
            text(value)
                .text_size(px(21.0))
                .font_weight(FontWeight::BOLD)
                .tabular_nums()
                .text_color(palette::text_primary()),
        )
        .child(
            text(label)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}

// --- Row 4 Right: Worth a look ----------------------------------------------------------

fn worth_a_look_card(snap: &DashboardSnapshot) -> impl IntoElement {
    let mut items = view().gap(px(8.0));
    if snap.insights.is_empty() {
        let empty_msg = if snap.has_activity() {
            "Everything looks healthy for this selection."
        } else {
            "No insights yet. Suggestions will appear here as workflows run."
        };
        items = items.child(
            text(empty_msg)
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    } else {
        for insight in &snap.insights {
            items = items.child(insight_row(insight));
        }
    }

    card()
        .flex_grow(1.0)
        .min_w(px(0.0))
        .p(px(16.0))
        .gap(px(12.0))
        .child(
            text("Worth a look")
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(items)
}

fn insight_row(insight: &DashboardInsight) -> impl IntoElement {
    view()
        .flex_row()
        .items_start()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(9.0))
        .bg(palette::subtle_surface_bg())
        .child(
            view()
                .pt(px(1.5))
                .child(icons::alert_circle(palette::warning_icon())),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(insight.headline.clone())
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(insight.detail.clone())
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
}

pub(crate) fn project_dot(color: ProjectColor) -> impl IntoElement {
    view()
        .w(px(8.0))
        .h(px(8.0))
        .rounded(px(2.0))
        .flex_shrink_0()
        .bg(palette::project_color(color))
}
