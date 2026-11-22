//! Native macOS Project Settings window with sidebar sections and placeholder data.
//!
//! Default `Issues & intake` pane matches `05 ProjectSettings.png`.

use factory_core::{
    IssueSourceAuthMode, IssueSourceKind, ProjectIntakeRule, ProjectSandboxMode,
    ProjectSettingsTab, ProviderKind,
};
use nsh_base::{button, input};
use nsh_core::{
    desktop::{TitleBarOptions, TitleBarStyle, WindowAttributes, WindowKind, WindowPosition},
    element::desktop::{current_window, window},
    prelude::*,
};

use crate::{
    store,
    ui::{icons, palette},
};

/// Declarative secondary macOS window hosting per-project settings.
pub(crate) fn project_settings_window() -> impl IntoElement {
    let mut title_bar = TitleBarOptions::default();
    title_bar.style = TitleBarStyle::Hidden;
    title_bar.height = Some(px(38.0));

    let mut attrs = WindowAttributes::default();
    attrs.title_bar = Some(title_bar);
    attrs.resizable = Some(true);
    attrs.min_size = Some(Some(Size::new(px(780.0), px(580.0))));
    attrs.close_on_escape = Some(true);

    window()
        .id("project-settings")
        .kind(WindowKind::Dialog)
        .parent("main")
        .default_size(Size::new(px(940.0), px(720.0)))
        .default_position(WindowPosition::ParentCenter)
        .title(move || store::active_project_settings().window_title())
        .attributes(attrs)
        .open(store::project_settings_open)
        .on_open_change(|change| {
            if !change.open {
                store::close_project_settings();
            }
        })
        .content(project_settings_surface)
}

fn project_settings_surface() -> impl IntoElement {
    store::set_project_settings_focused(true);
    current_window().on_focus_change(store::set_project_settings_focused);

    view()
        .flex_1()
        .min_h(px(0.0))
        .bg(palette::canvas_bg())
        .text_color(palette::text_primary())
        .child(title_bar_header())
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .flex_row()
                .child(settings_sidebar())
                .child(settings_content_pane()),
        )
}

// --- Window Header & Left Sidebar -------------------------------------------------------

fn title_bar_header() -> impl IntoElement {
    view()
        .h(px(38.0))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .bg(palette::header_bg())
        .border_b_1()
        .border_color(palette::border_color())
        .window_drag(true)
        .child(
            text(move || store::active_project_settings().window_title())
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
}

fn settings_sidebar() -> impl IntoElement {
    let mut rail = view()
        .w(px(196.0))
        .flex_shrink_0()
        .bg(palette::sidebar_bg())
        .border_r_1()
        .border_color(palette::border_color())
        .px(px(10.0))
        .py(px(12.0))
        .gap(px(2.0));

    for tab in ProjectSettingsTab::ALL {
        rail = rail.child(sidebar_tab_item(*tab));
    }

    rail
}

fn sidebar_tab_item(tab: ProjectSettingsTab) -> impl IntoElement {
    let is_active = move || store::project_settings_tab() == tab;
    let test_id = match tab {
        ProjectSettingsTab::General => "ps-tab-general",
        ProjectSettingsTab::IssuesAndIntake => "ps-tab-issues-intake",
        ProjectSettingsTab::Providers => "ps-tab-providers",
        ProjectSettingsTab::Workflows => "ps-tab-workflows",
        ProjectSettingsTab::Environment => "ps-tab-environment",
        ProjectSettingsTab::AgentsAndLimits => "ps-tab-agents-limits",
    };

    button()
        .test_id(test_id)
        .accessibility_label(tab.label())
        .on_press(move || store::set_project_settings_tab(tab))
        .w_full()
        .rounded(px(7.0))
        .px(px(10.0))
        .py(px(7.0))
        .items_start()
        .bg(move || {
            if is_active() {
                palette::nav_active_bg()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(move |s| {
            if is_active() {
                s
            } else {
                s.bg(palette::sidebar_hover_bg())
            }
        })
        .active(|s| s.opacity(0.85))
        .child(
            text(tab.label())
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if is_active() {
                        palette::nav_active_fg()
                    } else {
                        palette::text_primary()
                    }
                }),
        )
}

fn settings_content_pane() -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .bg(palette::canvas_bg())
        .child(when(
            || store::project_settings_tab() == ProjectSettingsTab::General,
            general_tab,
        ))
        .child(when(
            || store::project_settings_tab() == ProjectSettingsTab::IssuesAndIntake,
            issues_and_intake_tab,
        ))
        .child(when(
            || store::project_settings_tab() == ProjectSettingsTab::Providers,
            providers_tab,
        ))
        .child(when(
            || store::project_settings_tab() == ProjectSettingsTab::Workflows,
            workflows_tab,
        ))
        .child(when(
            || store::project_settings_tab() == ProjectSettingsTab::Environment,
            environment_tab,
        ))
        .child(when(
            || store::project_settings_tab() == ProjectSettingsTab::AgentsAndLimits,
            agents_and_limits_tab,
        ))
}

// --- Issues & Intake Tab (matching 05 ProjectSettings.png) ------------------------------

fn issues_and_intake_tab() -> impl IntoElement {
    let token_editor_open = signal(false);
    let draft_token = signal(String::new());
    let add_rule_open = signal(false);
    let draft_rule_label = signal(String::new());
    let draft_rule_workflow = signal("Fix issue".to_owned());

    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .px(px(26.0))
            .py(px(22.0))
            .gap(px(20.0))
            .child(issue_source_section(token_editor_open, draft_token))
            .child(intake_rules_section(
                add_rule_open,
                draft_rule_label,
                draft_rule_workflow,
            ))
            .child(auto_start_card())
            .child(update_issue_fieldset())
            .child(settings_messages_strip()),
    )
}

fn issue_source_section(
    token_editor_open: Signal<bool>,
    draft_token: Signal<String>,
) -> impl IntoElement {
    view()
        .gap(px(12.0))
        .child(section_heading(
            "Issue source",
            "Where this project's work comes from. Each project uses one source.",
        ))
        .child(dynamic(move || {
            let selected_source = store::active_project_settings().issue_source;
            view()
                .flex_row()
                .gap(px(12.0))
                .child(source_option_card(
                    "ps-source-github",
                    IssueSourceKind::GitHubIssues,
                    selected_source == IssueSourceKind::GitHubIssues,
                    false,
                    token_editor_open,
                ))
                .child(source_option_card(
                    "ps-source-itsaplan",
                    IssueSourceKind::ItsAPlan,
                    selected_source == IssueSourceKind::ItsAPlan,
                    false,
                    token_editor_open,
                ))
                .child(source_option_card(
                    "ps-source-extension",
                    IssueSourceKind::Extension,
                    selected_source == IssueSourceKind::Extension,
                    true,
                    token_editor_open,
                ))
        }))
        .child(source_details_card(token_editor_open, draft_token))
}

fn source_option_card(
    test_id: &'static str,
    kind: IssueSourceKind,
    selected: bool,
    disabled: bool,
    token_editor_open: Signal<bool>,
) -> impl IntoElement {
    let bg = if selected {
        palette::card_selected_bg()
    } else {
        palette::card_bg()
    };
    let border = if selected {
        palette::card_selected_border()
    } else {
        palette::border_color()
    };
    let title_color = if disabled {
        palette::text_muted()
    } else {
        palette::text_primary()
    };

    button()
        .test_id(test_id)
        .accessibility_label(kind.title())
        .disabled(disabled)
        .on_press(move || {
            token_editor_open.set(false);
            store::set_project_issue_source(kind);
        })
        .flex_1()
        .min_w(px(0.0))
        .p(px(14.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .items_start()
        .gap(px(8.0))
        .hover(move |s| {
            if selected || disabled {
                s
            } else {
                s.bg(palette::card_inset_bg())
            }
        })
        .active(|s| s.opacity(0.9))
        .disabled_style(|s| s.opacity(0.65))
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            view()
                                .w(px(24.0))
                                .h(px(24.0))
                                .rounded(px(6.0))
                                .flex_shrink_0()
                                .items_center()
                                .justify_center()
                                .bg(palette::provider_badge_bg())
                                .child(
                                    text(kind.badge())
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(title_color),
                                ),
                        )
                        .child(
                            text(kind.title())
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(title_color),
                        ),
                )
                .child(radio_circle(selected, disabled)),
        )
        .child(
            text(kind.description())
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}

fn radio_circle(selected: bool, disabled: bool) -> impl IntoElement {
    let outer = view()
        .w(px(16.0))
        .h(px(16.0))
        .rounded_full()
        .flex_shrink_0()
        .items_center()
        .justify_center();

    if selected {
        outer.bg(palette::nav_active_bg()).child(
            view()
                .w(px(6.0))
                .h(px(6.0))
                .rounded_full()
                .bg(palette::nav_active_fg()),
        )
    } else {
        outer
            .border_1()
            .border_color(if disabled {
                palette::divider_color()
            } else {
                palette::border_color()
            })
            .bg(palette::card_inset_bg())
    }
}

fn source_details_card(
    token_editor_open: Signal<bool>,
    draft_token: Signal<String>,
) -> impl IntoElement {
    view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(when(
            || store::active_project_settings().issue_source == IssueSourceKind::GitHubIssues,
            move || github_source_rows(token_editor_open, draft_token),
        ))
        .child(when(
            || store::active_project_settings().issue_source == IssueSourceKind::ItsAPlan,
            itsaplan_source_rows,
        ))
        .child(card_divider())
        .child(dynamic(|| {
            let status_text = store::active_project_settings().source_status_text();
            let status_ok = status_text.starts_with("Connected");
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(16.0))
                .py(px(11.0))
                .child(form_row_label("Status"))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(when(
                            move || status_ok,
                            || icons::check(palette::success_text()),
                        ))
                        .child(
                            text(status_text)
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if status_ok {
                                    palette::success_text()
                                } else {
                                    palette::warning_text()
                                }),
                        ),
                )
        }))
}

fn github_source_rows(
    token_editor_open: Signal<bool>,
    draft_token: Signal<String>,
) -> impl IntoElement {
    view()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(16.0))
                .py(px(10.0))
                .child(form_row_label("Repository"))
                .child(
                    input()
                        .test_id("ps-github-repo")
                        .flex_1()
                        .value(|| store::active_project_settings().github_repo)
                        .on_change(store::set_project_github_repo)
                        .placeholder("owner/repository")
                        .app_class("font-mono")
                        .px(px(10.0))
                        .py(px(6.5))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_inset_bg())
                        .text_size(px(12.5))
                        .text_color(palette::text_primary()),
                ),
        )
        .child(card_divider())
        .child(
            view()
                .px(px(16.0))
                .py(px(10.0))
                .gap(px(10.0))
                .child(dynamic(move || {
                    let settings = store::active_project_settings();
                    let sign_in_summary = settings.sign_in_summary();
                    let auth_mode = settings.github_auth_mode;
                    let show_token_editor = token_editor_open.get();

                    view()
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
                                .gap(px(12.0))
                                .child(form_row_label("Sign-in"))
                                .child(
                                    text(sign_in_summary)
                                        .text_size(px(13.0))
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(match auth_mode {
                            IssueSourceAuthMode::GitHubCli => secondary_button(
                                "ps-use-token-btn",
                                "Use a Token Instead...",
                                move || {
                                    store::clear_project_settings_messages();
                                    draft_token.set(String::new());
                                    token_editor_open.set(!show_token_editor);
                                },
                            )
                            .into_element(),
                            IssueSourceAuthMode::PersonalToken => view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(secondary_button(
                                    "ps-replace-token-btn",
                                    "Replace Token...",
                                    move || {
                                        store::clear_project_settings_messages();
                                        draft_token.set(String::new());
                                        token_editor_open.set(!show_token_editor);
                                    },
                                ))
                                .child(secondary_button(
                                    "ps-use-gh-cli-btn",
                                    "Use GitHub CLI",
                                    move || {
                                        token_editor_open.set(false);
                                        store::use_project_github_cli();
                                    },
                                ))
                                .into_element(),
                        })
                }))
                .child(when(
                    move || token_editor_open.get(),
                    move || {
                        let save_token = move || {
                            let raw = draft_token.get_untracked();
                            if store::set_project_github_token(&raw) {
                                draft_token.set(String::new());
                                token_editor_open.set(false);
                            }
                        };
                        view()
                            .pl(px(152.0))
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                input()
                                    .test_id("ps-token-input")
                                    .flex_1()
                                    .value(move || draft_token.get())
                                    .on_change(move |v: &str| draft_token.set(v.to_owned()))
                                    .placeholder("ghp_...")
                                    .app_class("font-mono")
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .rounded(px(7.0))
                                    .border_1()
                                    .border_color(palette::border_color())
                                    .bg(palette::card_inset_bg())
                                    .text_size(px(12.5))
                                    .text_color(palette::text_primary()),
                            )
                            .child(secondary_button("ps-token-cancel", "Cancel", move || {
                                store::clear_project_settings_messages();
                                token_editor_open.set(false);
                            }))
                            .child(primary_button("ps-token-save", "Save Token", save_token))
                    },
                )),
        )
}

fn itsaplan_source_rows() -> impl IntoElement {
    view()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(16.0))
                .py(px(10.0))
                .child(form_row_label("Workspace"))
                .child(
                    input()
                        .test_id("ps-itsaplan-workspace")
                        .flex_1()
                        .value(|| store::active_project_settings().itsaplan_workspace)
                        .on_change(store::set_project_itsaplan_workspace)
                        .placeholder("workspace-slug")
                        .app_class("font-mono")
                        .px(px(10.0))
                        .py(px(6.5))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_inset_bg())
                        .text_size(px(12.5))
                        .text_color(palette::text_primary()),
                ),
        )
        .child(card_divider())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(16.0))
                .py(px(10.0))
                .child(form_row_label("Agent member"))
                .child(
                    input()
                        .test_id("ps-itsaplan-member")
                        .flex_1()
                        .value(|| store::active_project_settings().itsaplan_agent_member)
                        .on_change(store::set_project_itsaplan_agent_member)
                        .placeholder("@factory-bot")
                        .app_class("font-mono")
                        .px(px(10.0))
                        .py(px(6.5))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_inset_bg())
                        .text_size(px(12.5))
                        .text_color(palette::text_primary()),
                ),
        )
        .child(card_divider())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(16.0))
                .py(px(11.0))
                .child(form_row_label("Sign-in"))
                .child(
                    text(|| store::active_project_settings().sign_in_summary())
                        .text_size(px(13.0))
                        .text_color(palette::text_primary()),
                ),
        )
}

fn intake_rules_section(
    add_rule_open: Signal<bool>,
    draft_rule_label: Signal<String>,
    draft_rule_workflow: Signal<String>,
) -> impl IntoElement {
    let concurrency_draft = signal(
        store::active_project_settings()
            .max_concurrent_runs
            .to_string(),
    );

    view()
        .gap(px(12.0))
        .child(
            view()
                .gap(px(3.0))
                .child(
                    text("Intake rules")
                        .text_size(px(14.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(|| store::active_project_settings().intake_rules_subtitle())
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(dynamic(move || {
            let settings = store::active_project_settings();
            let show_remove = add_rule_open.get();

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
                        .px(px(16.0))
                        .py(px(9.0))
                        .bg(palette::card_inset_bg())
                        .child(
                            view().w(px(56.0)).flex_shrink_0().child(
                                text("Order")
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_muted()),
                            ),
                        )
                        .child(
                            view().flex_1().min_w(px(0.0)).child(
                                text("When the issue")
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_muted()),
                            ),
                        )
                        .child(
                            view().w(px(230.0)).flex_shrink_0().child(
                                text("Run workflow")
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_muted()),
                            ),
                        ),
                );

            if settings.intake_rules.is_empty() {
                table = table.child(card_divider()).child(
                    view().px(px(16.0)).py(px(14.0)).child(
                        text("No intake rules configured yet. Add a rule to route labeled issues to a workflow.")
                            .text_size(px(12.5))
                            .text_color(palette::text_muted()),
                    ),
                );
            } else {
                for (idx, rule) in settings.intake_rules.iter().enumerate() {
                    table = table
                        .child(card_divider())
                        .child(intake_rule_row(idx + 1, rule, show_remove));
                }
            }
            table
        }))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(secondary_button("ps-add-rule-btn", "Add Rule", move || {
                    store::clear_project_settings_messages();
                    draft_rule_label.set(String::new());
                    draft_rule_workflow.set("Fix issue".to_owned());
                    add_rule_open.update(|open| *open = !*open);
                }))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            text("Runs at once for this project")
                                .text_size(px(12.5))
                                .text_color(palette::text_muted()),
                        )
                        .child(
                            input()
                                .test_id("ps-concurrency-input")
                                .w(px(62.0))
                                .value(move || concurrency_draft.get())
                                .on_change(move |v: &str| {
                                    concurrency_draft.set(v.to_owned());
                                    if !v.trim().is_empty() {
                                        store::set_project_max_concurrent_runs_str(v);
                                    }
                                })
                                .px(px(10.0))
                                .py(px(5.5))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_inset_bg())
                                .text_size(px(12.5))
                                .tabular_nums()
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
        .child(when(
            move || add_rule_open.get(),
            move || {
                let save_rule = move || {
                    let label = draft_rule_label.get_untracked();
                    let workflow = draft_rule_workflow.get_untracked();
                    if store::add_project_intake_rule(&label, &workflow) {
                        draft_rule_label.set(String::new());
                        add_rule_open.set(false);
                    }
                };

                view()
                    .p(px(14.0))
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_bg())
                    .gap(px(10.0))
                    .child(
                        text("Add intake rule")
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_secondary()),
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
                                        text("Issue label")
                                            .text_size(px(11.5))
                                            .text_color(palette::text_muted()),
                                    )
                                    .child(
                                        input()
                                            .test_id("ps-new-rule-label")
                                            .value(move || draft_rule_label.get())
                                            .on_change(move |v: &str| {
                                                draft_rule_label.set(v.to_owned());
                                            })
                                            .placeholder("e.g. security")
                                            .px(px(10.0))
                                            .py(px(6.0))
                                            .rounded(px(7.0))
                                            .border_1()
                                            .border_color(palette::border_color())
                                            .bg(palette::card_inset_bg())
                                            .text_size(px(12.5))
                                            .text_color(palette::text_primary()),
                                    ),
                            )
                            .child(
                                view()
                                    .flex_1()
                                    .gap(px(4.0))
                                    .child(
                                        text("Run workflow")
                                            .text_size(px(11.5))
                                            .text_color(palette::text_muted()),
                                    )
                                    .child(
                                        input()
                                            .test_id("ps-new-rule-workflow")
                                            .value(move || draft_rule_workflow.get())
                                            .on_change(move |v: &str| {
                                                draft_rule_workflow.set(v.to_owned());
                                            })
                                            .placeholder("e.g. Fix issue")
                                            .px(px(10.0))
                                            .py(px(6.0))
                                            .rounded(px(7.0))
                                            .border_1()
                                            .border_color(palette::border_color())
                                            .bg(palette::card_inset_bg())
                                            .text_size(px(12.5))
                                            .text_color(palette::text_primary()),
                                    ),
                            ),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(secondary_button("ps-rule-cancel", "Cancel", move || {
                                store::clear_project_settings_messages();
                                add_rule_open.set(false);
                            }))
                            .child(primary_button("ps-rule-save", "Save Rule", save_rule)),
                    )
            },
        ))
}

fn intake_rule_row(order: usize, rule: &ProjectIntakeRule, show_remove: bool) -> impl IntoElement {
    let rule_id = rule.id;
    let can_remove = !rule.is_fallback() && (show_remove || rule.id >= 4);

    view()
        .flex_row()
        .items_center()
        .px(px(16.0))
        .py(px(11.0))
        .child(
            view().w(px(56.0)).flex_shrink_0().child(
                text(order.to_string())
                    .text_size(px(13.0))
                    .tabular_nums()
                    .text_color(palette::text_muted()),
            ),
        )
        .child(
            view().flex_1().min_w(px(0.0)).child(
                text(rule.condition_text())
                    .text_size(px(13.0))
                    .text_color(palette::text_primary()),
            ),
        )
        .child(
            view()
                .w(px(230.0))
                .flex_shrink_0()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    text(rule.workflow_name.clone())
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(when(
                    move || can_remove,
                    move || {
                        let remove_id = format!("ps-remove-rule-{rule_id}");
                        button()
                            .test_id(&remove_id)
                            .accessibility_label(format!("Remove rule {order}"))
                            .on_press(move || store::remove_project_intake_rule(rule_id))
                            .px(px(7.0))
                            .py(px(2.5))
                            .rounded(px(5.0))
                            .hover(|s| s.bg(palette::subtle_surface_bg()))
                            .child(
                                text("Remove")
                                    .text_size(px(11.0))
                                    .text_color(palette::destructive_text()),
                            )
                    },
                )),
        )
}

fn auto_start_card() -> impl IntoElement {
    dynamic(|| {
        let checked = store::active_project_settings().auto_start_matching_work;
        view()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .px(px(16.0))
            .py(px(13.0))
            .rounded(px(10.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::card_bg())
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(2.0))
                    .child(
                        text("Start matching work automatically")
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_primary()),
                    )
                    .child(
                        text("Off: issues wait in Ready for agents until you press Start.")
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                    ),
            )
            .child(macos_checkbox(
                "ps-auto-start-checkbox",
                "Start matching work automatically",
                checked,
                store::toggle_project_auto_start,
            ))
    })
}

fn update_issue_fieldset() -> impl IntoElement {
    dynamic(|| {
        let settings = store::active_project_settings();
        view()
            .relative()
            .pt(px(9.0))
            .child(
                view()
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_bg())
                    .px(px(16.0))
                    .pt(px(18.0))
                    .pb(px(14.0))
                    .gap(px(11.0))
                    .child(checkbox_line(
                        "ps-update-working-label",
                        "Add the label agent-working while a run is active",
                        settings.add_working_label,
                        store::toggle_project_add_working_label,
                    ))
                    .child(checkbox_line(
                        "ps-update-comment-summary",
                        "Comment with the run link and the final summary",
                        settings.comment_with_run_summary,
                        store::toggle_project_comment_summary,
                    ))
                    .child(checkbox_line(
                        "ps-update-link-pr",
                        "Link the pull request so merging closes the issue",
                        settings.link_pr_closes_issue,
                        store::toggle_project_link_pr_closes_issue,
                    )),
            )
            .child(
                view()
                    .absolute()
                    .top(px(0.0))
                    .left(px(12.0))
                    .px(px(6.0))
                    .bg(palette::canvas_bg())
                    .child(
                        text("Update the issue as work moves")
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_primary()),
                    ),
            )
    })
}

// --- General Tab ------------------------------------------------------------------------

fn general_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .px(px(26.0))
            .py(px(22.0))
            .gap(px(18.0))
            .child(dynamic(|| {
                let settings = store::active_project_settings();

                view()
                    .gap(px(18.0))
                    .child(section_heading(
                        "General",
                        "Repository location, branch conventions, and worktree lifecycle for this project.",
                    ))
                    .child(
                        grouped_card("Repository")
                            .child(card_divider())
                            .child(static_row("Project name", settings.project_name.clone(), false))
                            .child(card_divider())
                            .child(static_row("Local path", settings.root_path.clone(), true))
                            .child(card_divider())
                            .child(static_row("Git remote", settings.git_remote.clone(), true))
                            .child(card_divider())
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(12.0))
                                    .px(px(16.0))
                                    .py(px(10.0))
                                    .child(form_row_label("Default branch"))
                                    .child(
                                        input()
                                            .test_id("ps-general-branch")
                                            .flex_1()
                                            .value(settings.default_branch.clone())
                                            .on_change(store::set_project_default_branch)
                                            .app_class("font-mono")
                                            .px(px(10.0))
                                            .py(px(6.0))
                                            .rounded(px(7.0))
                                            .border_1()
                                            .border_color(palette::border_color())
                                            .bg(palette::card_inset_bg())
                                            .text_size(px(12.5))
                                            .text_color(palette::text_primary()),
                                    ),
                            ),
                    )
                    .child(
                        grouped_card("Branches & worktrees")
                            .child(card_divider())
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(12.0))
                                    .px(px(16.0))
                                    .py(px(10.0))
                                    .child(form_row_label("Branch prefix"))
                                    .child(
                                        input()
                                            .test_id("ps-general-prefix")
                                            .flex_1()
                                            .value(settings.branch_prefix.clone())
                                            .on_change(store::set_project_branch_prefix)
                                            .app_class("font-mono")
                                            .px(px(10.0))
                                            .py(px(6.0))
                                            .rounded(px(7.0))
                                            .border_1()
                                            .border_color(palette::border_color())
                                            .bg(palette::card_inset_bg())
                                            .text_size(px(12.5))
                                            .text_color(palette::text_primary()),
                                    ),
                            )
                            .child(card_divider())
                            .child(toggle_row(
                                "Run workflows in isolated git worktrees",
                                "Each run checks out a clean worktree so local edits in your primary checkout are untouched.",
                                settings.use_isolated_worktrees,
                                store::toggle_project_isolated_worktrees,
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Open pull requests as drafts by default",
                                "Mark new agent pull requests as draft until you review them.",
                                settings.open_prs_as_draft,
                                store::toggle_project_draft_prs,
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Clean up worktrees after successful runs",
                                "Automatically remove temporary worktrees once a run completes cleanly.",
                                settings.auto_cleanup_worktrees,
                                store::toggle_project_auto_cleanup_worktrees,
                            )),
                    )
                    .child(
                        grouped_card("Project status")
                            .child(card_divider())
                            .child(setting_row(
                                if settings.archived {
                                    "Project is archived"
                                } else {
                                    "Archive project"
                                },
                                "Archived projects stop automatic intake and stay read-only in run history.",
                                button()
                                    .test_id("ps-general-archive")
                                    .on_press(store::toggle_project_archived)
                                    .px(px(12.0))
                                    .py(px(5.5))
                                    .rounded(px(7.0))
                                    .border_1()
                                    .border_color(palette::border_color())
                                    .bg(palette::control_bg())
                                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                                    .active(|s| s.opacity(0.8))
                                    .child(
                                        text(if settings.archived {
                                            "Restore Project"
                                        } else {
                                            "Archive Project..."
                                        })
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if settings.archived {
                                            palette::text_primary()
                                        } else {
                                            palette::destructive_text()
                                        }),
                                    ),
                            )),
                    )
                    .child(settings_messages_strip())
            })),
    )
}

// --- Providers Tab ----------------------------------------------------------------------

fn providers_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .px(px(26.0))
            .py(px(22.0))
            .gap(px(18.0))
            .child(dynamic(|| {
                let settings = store::active_project_settings();
                let prefs = store::preferences();

                let provider_picker = view()
                    .flex_row()
                    .p(px(2.5))
                    .rounded(px(8.0))
                    .bg(palette::segmented_track_bg())
                    .gap(px(2.0))
                    .child(segment_button(
                        "Claude",
                        settings.default_provider == ProviderKind::Claude,
                        || store::set_project_default_provider(ProviderKind::Claude),
                    ))
                    .child(segment_button(
                        "Codex",
                        settings.default_provider == ProviderKind::Codex,
                        || store::set_project_default_provider(ProviderKind::Codex),
                    ))
                    .child(segment_button(
                        "OpenRouter",
                        settings.default_provider == ProviderKind::OpenRouter,
                        || store::set_project_default_provider(ProviderKind::OpenRouter),
                    ));

                let mut or_picker = view().flex_row().flex_wrap().gap(px(6.0));
                or_picker = or_picker.child(pill_choice(
                    "Workspace default",
                    settings.openrouter_key_id.is_none(),
                    || store::set_project_openrouter_key(None),
                ));
                for key in &prefs.openrouter_keys {
                    let key_id = key.id.clone();
                    let active = settings.openrouter_key_id.as_deref() == Some(key.id.as_str());
                    or_picker = or_picker.child(pill_choice(
                        format!("{} (••{})", key.name, key.key_last4),
                        active,
                        move || store::set_project_openrouter_key(Some(key_id.clone())),
                    ));
                }

                let selected_or_summary = settings
                    .openrouter_key_id
                    .as_deref()
                    .and_then(|id| prefs.openrouter_key(id))
                    .map(|k| format!("{} · {}", k.name, k.cap_summary()))
                    .unwrap_or_else(|| {
                        if prefs.openrouter_keys.is_empty() {
                            "No OpenRouter keys configured".to_owned()
                        } else {
                            "Workspace default OpenRouter key".to_owned()
                        }
                    });

                view()
                    .gap(px(18.0))
                    .child(section_heading(
                        "Providers",
                        "Choose which provider accounts and OpenRouter key this project uses by default.",
                    ))
                    .child(
                        grouped_card("Routing & credentials")
                            .child(card_divider())
                            .child(setting_row(
                                "Default provider",
                                "Used when a workflow step does not pin an explicit provider.",
                                provider_picker,
                            ))
                            .child(card_divider())
                            .child(
                                view()
                                    .px(px(16.0))
                                    .py(px(12.0))
                                    .gap(px(8.0))
                                    .child(
                                        view()
                                            .gap(px(2.0))
                                            .child(
                                                text("OpenRouter key")
                                                    .text_size(px(13.0))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(palette::text_primary()),
                                            )
                                            .child(
                                                text("Workflow files name a provider, never a key. Pick which Keychain key bills this repository.")
                                                    .text_size(px(11.5))
                                                    .text_color(palette::text_muted()),
                                            ),
                                    )
                                    .child(or_picker),
                            )
                            .child(card_divider())
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(12.0))
                                    .px(px(16.0))
                                    .py(px(10.0))
                                    .child(form_row_label("Model override"))
                                    .child(
                                        input()
                                            .test_id("ps-providers-model")
                                            .flex_1()
                                            .value(settings.model_override.clone())
                                            .on_change(store::set_project_model_override)
                                            .placeholder("Inherit provider default (e.g. claude-sonnet-4-5)")
                                            .app_class("font-mono")
                                            .px(px(10.0))
                                            .py(px(6.0))
                                            .rounded(px(7.0))
                                            .border_1()
                                            .border_color(palette::border_color())
                                            .bg(palette::card_inset_bg())
                                            .text_size(px(12.5))
                                            .text_color(palette::text_primary()),
                                    ),
                            ),
                    )
                    .child(
                        grouped_card("Available accounts")
                            .child(card_divider())
                            .child(static_row(
                                "Claude CLI",
                                format!("{} · Inherited from Preferences", prefs.claude.list_subtitle()),
                                false,
                            ))
                            .child(card_divider())
                            .child(static_row(
                                "Codex CLI",
                                format!("{} · Inherited from Preferences", prefs.codex.list_subtitle()),
                                false,
                            ))
                            .child(card_divider())
                            .child(static_row("OpenRouter", selected_or_summary, false)),
                    )
                    .child(
                        view().flex_row().justify_end().child(secondary_button(
                            "ps-open-provider-prefs",
                            "Open Provider Preferences...",
                            || {
                                store::open_preferences_tab(
                                    factory_core::PreferencesTab::Providers,
                                );
                            },
                        )),
                    )
            })),
    )
}

// --- Workflows Tab ----------------------------------------------------------------------

fn workflows_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .px(px(26.0))
            .py(px(22.0))
            .gap(px(18.0))
            .child(dynamic(|| {
                let settings = store::active_project_settings();

                let mut list_card = grouped_card("Discovered workflows");
                if settings.workflows.is_empty() {
                    list_card = list_card.child(card_divider()).child(
                        view().px(px(16.0)).py(px(14.0)).child(
                            text("No workflow files discovered yet. Add definitions in .factory/workflows/ or create one from Workflows.")
                                .text_size(px(12.5))
                                .text_color(palette::text_muted()),
                        ),
                    );
                } else {
                    for wf in &settings.workflows {
                        let wf_name = wf.name.clone();
                        let wf_toggle_name = wf_name.clone();
                        let subtitle =
                            format!("{} · {} · {}", wf.file_path, wf.trigger_label, wf.summary);
                        list_card = list_card.child(card_divider()).child(toggle_row(
                            wf_name,
                            subtitle,
                            wf.enabled,
                            move || store::toggle_project_workflow(&wf_toggle_name),
                        ));
                    }
                }

                view()
                    .gap(px(18.0))
                    .child(section_heading(
                        "Workflows",
                        "Workflow definitions loaded from this repository and available to intake rules or ad-hoc runs.",
                    ))
                    .child(list_card)
                    .child(
                        grouped_card("Execution defaults")
                            .child(card_divider())
                            .child(static_row(
                                "Workflows folder",
                                settings.workflows_dir.clone(),
                                true,
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Require review gate before opening pull request",
                                "Pause runs for human sign-off before pushing branches or opening PRs.",
                                settings.require_review_before_pr,
                                store::toggle_project_require_review_before_pr,
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Reload workflow files automatically",
                                "Watch .factory/workflows/ and pick up YAML edits without restarting.",
                                settings.auto_reload_workflows,
                                store::toggle_project_auto_reload_workflows,
                            )),
                    )
            })),
    )
}

// --- Environment Tab --------------------------------------------------------------------

fn environment_tab() -> impl IntoElement {
    let add_var_open = signal(false);
    let draft_key = signal(String::new());
    let draft_value = signal(String::new());
    let draft_secret = signal(false);

    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .px(px(26.0))
            .py(px(22.0))
            .gap(px(18.0))
            .child(dynamic(move || {
                let settings = store::active_project_settings();
                let show_add = add_var_open.get();

                let mut vars_card = view()
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_bg())
                    .overflow_hidden()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .px(px(16.0))
                            .py(px(9.0))
                            .bg(palette::card_inset_bg())
                            .child(
                                view().w(px(210.0)).flex_shrink_0().child(
                                    text("Variable")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_muted()),
                                ),
                            )
                            .child(
                                view().flex_1().min_w(px(0.0)).child(
                                    text("Value")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_muted()),
                                ),
                            ),
                    );

                if settings.env_vars.is_empty() {
                    vars_card = vars_card.child(card_divider()).child(
                        view().px(px(16.0)).py(px(14.0)).child(
                            text("No project environment variables configured.")
                                .text_size(px(12.5))
                                .text_color(palette::text_muted()),
                        ),
                    );
                } else {
                    for env_var in &settings.env_vars {
                        let var_id = env_var.id;
                        let remove_id = format!("ps-env-remove-{var_id}");
                        vars_card = vars_card.child(card_divider()).child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .px(px(16.0))
                                .py(px(10.0))
                                .child(
                                    view()
                                        .w(px(210.0))
                                        .flex_shrink_0()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            text(env_var.key.clone())
                                                .app_class("font-mono")
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(palette::text_primary()),
                                        ),
                                )
                                .child(
                                    text(env_var.display_value())
                                        .flex_1()
                                        .app_class("font-mono")
                                        .text_size(px(12.5))
                                        .text_color(palette::text_secondary()),
                                )
                                .child(
                                    button()
                                        .test_id(&remove_id)
                                        .accessibility_label(format!("Remove {}", env_var.key))
                                        .on_press(move || store::remove_project_env_var(var_id))
                                        .px(px(8.0))
                                        .py(px(3.0))
                                        .rounded(px(5.0))
                                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                                        .child(
                                            text("Remove")
                                                .text_size(px(11.5))
                                                .text_color(palette::destructive_text()),
                                        ),
                                ),
                        );
                    }
                }

                let mut body = view()
                    .gap(px(18.0))
                    .child(section_heading(
                        "Environment",
                        "Environment variables passed to agent steps and local test commands. Secrets stay in the macOS Keychain.",
                    ))
                    .child(vars_card)
                    .child(
                        view().flex_row().items_center().justify_start().child(
                            secondary_button("ps-env-add-btn", "Add Variable", move || {
                                store::clear_project_settings_messages();
                                draft_key.set(String::new());
                                draft_value.set(String::new());
                                draft_secret.set(false);
                                add_var_open.set(!show_add);
                            }),
                        ),
                    );

                if show_add {
                    let save_var = move || {
                        let key = draft_key.get_untracked();
                        let value = draft_value.get_untracked();
                        let secret = draft_secret.get_untracked();
                        if store::add_project_env_var(&key, &value, secret) {
                            draft_key.set(String::new());
                            draft_value.set(String::new());
                            draft_secret.set(false);
                            add_var_open.set(false);
                        }
                    };
                    let is_secret = draft_secret.get();

                    body = body.child(
                        view()
                            .p(px(14.0))
                            .rounded(px(10.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_bg())
                            .gap(px(10.0))
                            .child(
                                view()
                                    .flex_row()
                                    .gap(px(10.0))
                                    .child(
                                        view().flex_1().gap(px(4.0)).child(
                                            text("Name")
                                                .text_size(px(11.5))
                                                .text_color(palette::text_muted()),
                                        ).child(
                                            input()
                                                .test_id("ps-env-key-input")
                                                .value(move || draft_key.get())
                                                .on_change(move |v: &str| draft_key.set(v.to_owned()))
                                                .placeholder("e.g. API_BASE_URL")
                                                .app_class("font-mono")
                                                .px(px(10.0))
                                                .py(px(6.0))
                                                .rounded(px(7.0))
                                                .border_1()
                                                .border_color(palette::border_color())
                                                .bg(palette::card_inset_bg())
                                                .text_size(px(12.5))
                                                .text_color(palette::text_primary()),
                                        ),
                                    )
                                    .child(
                                        view().flex_1().gap(px(4.0)).child(
                                            text("Value")
                                                .text_size(px(11.5))
                                                .text_color(palette::text_muted()),
                                        ).child(
                                            input()
                                                .test_id("ps-env-value-input")
                                                .value(move || draft_value.get())
                                                .on_change(move |v: &str| draft_value.set(v.to_owned()))
                                                .placeholder("value")
                                                .app_class("font-mono")
                                                .px(px(10.0))
                                                .py(px(6.0))
                                                .rounded(px(7.0))
                                                .border_1()
                                                .border_color(palette::border_color())
                                                .bg(palette::card_inset_bg())
                                                .text_size(px(12.5))
                                                .text_color(palette::text_primary()),
                                        ),
                                    ),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(checkbox_line(
                                        "ps-env-secret-checkbox",
                                        "Store as masked secret",
                                        is_secret,
                                        move || draft_secret.update(|s| *s = !*s),
                                    ))
                                    .child(
                                        view()
                                            .flex_row()
                                            .gap(px(8.0))
                                            .child(secondary_button(
                                                "ps-env-cancel",
                                                "Cancel",
                                                move || {
                                                    store::clear_project_settings_messages();
                                                    add_var_open.set(false);
                                                },
                                            ))
                                            .child(primary_button(
                                                "ps-env-save",
                                                "Save Variable",
                                                save_var,
                                            )),
                                    ),
                            ),
                    );
                }

                body.child(
                    grouped_card("Shell & dotenv")
                        .child(card_divider())
                        .child(toggle_row(
                            "Load .env.local from repository root",
                            "Automatically read untracked .env.local files before running workflow commands.",
                            settings.load_dotenv_local,
                            store::toggle_project_load_dotenv,
                        ))
                        .child(card_divider())
                        .child(toggle_row(
                            "Inherit host PATH and toolchain shims",
                            "Expose Homebrew, rustup, and nvm binaries from your login shell.",
                            settings.inherit_host_path,
                            store::toggle_project_inherit_host_path,
                        )),
                )
                .child(settings_messages_strip())
            })),
    )
}

// --- Agents & Limits Tab ----------------------------------------------------------------

fn agents_and_limits_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .px(px(26.0))
            .py(px(22.0))
            .gap(px(18.0))
            .child(dynamic(|| {
                let settings = store::active_project_settings();

                let mut sandbox_picker = view()
                    .flex_row()
                    .p(px(2.5))
                    .rounded(px(8.0))
                    .bg(palette::segmented_track_bg())
                    .gap(px(2.0));
                for mode in ProjectSandboxMode::ALL {
                    let mode_val = *mode;
                    sandbox_picker = sandbox_picker.child(segment_button(
                        mode.label(),
                        settings.sandbox_mode == mode_val,
                        move || store::set_project_sandbox_mode(mode_val),
                    ));
                }

                let mut runner_picker = view()
                    .flex_row()
                    .p(px(2.5))
                    .rounded(px(8.0))
                    .bg(palette::segmented_track_bg())
                    .gap(px(2.0));
                for runner in ["Auto", "Mac Studio", "Linux builder"] {
                    runner_picker = runner_picker.child(segment_button(
                        runner,
                        settings.preferred_runner == runner,
                        move || store::set_project_preferred_runner(runner),
                    ));
                }

                view()
                    .gap(px(18.0))
                    .child(section_heading(
                        "Agents & limits",
                        "Concurrency caps, retry bounds, budget guardrails, and sandbox permissions for this project.",
                    ))
                    .child(
                        grouped_card("Concurrency & timeouts")
                            .child(card_divider())
                            .child(setting_row(
                                "Runs at once for this project",
                                "Maximum parallel workflow runs scheduled in this repository.",
                                stepper_control(
                                    settings.max_concurrent_runs.to_string(),
                                    "Decrease project concurrent runs",
                                    "Increase project concurrent runs",
                                    || store::step_project_max_concurrent_runs(-1),
                                    || store::step_project_max_concurrent_runs(1),
                                ),
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Max loop attempts per step",
                                "Retry limit for fix-until-tests-pass loops before pausing for human help.",
                                stepper_control(
                                    settings.max_loop_attempts.to_string(),
                                    "Decrease max loop attempts",
                                    "Increase max loop attempts",
                                    || store::step_project_max_loop_attempts(-1),
                                    || store::step_project_max_loop_attempts(1),
                                ),
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Step timeout",
                                "Cancel a hung agent step if no tool output arrives within this window.",
                                stepper_control(
                                    format!("{} min", settings.step_timeout_minutes),
                                    "Decrease step timeout",
                                    "Increase step timeout",
                                    || store::step_project_step_timeout(-5),
                                    || store::step_project_step_timeout(5),
                                ),
                            )),
                    )
                    .child(
                        grouped_card("Budget & guardrails")
                            .child(card_divider())
                            .child(setting_row(
                                "Per-run budget limit",
                                "Budget Guard threshold for an individual workflow run.",
                                stepper_control(
                                    settings.formatted_per_run_budget(),
                                    "Decrease per-run budget",
                                    "Increase per-run budget",
                                    || store::step_project_per_run_budget(-100),
                                    || store::step_project_per_run_budget(100),
                                ),
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Monthly project budget",
                                "Soft spending target tracked across all runs in this repository.",
                                stepper_control(
                                    settings.formatted_monthly_budget(),
                                    "Decrease monthly budget",
                                    "Increase monthly budget",
                                    || store::step_project_monthly_budget(-1_000),
                                    || store::step_project_monthly_budget(1_000),
                                ),
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Pause run when per-run budget reaches 100%",
                                "Ask for approval in Inbox before continuing past the per-run cap.",
                                settings.pause_on_budget_limit,
                                store::toggle_project_pause_on_budget,
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Require approval before editing CI or deploy config",
                                "Prompt before agents change workflow pipelines or deployment manifests.",
                                settings.require_approval_for_ci,
                                store::toggle_project_require_approval_for_ci,
                            )),
                    )
                    .child(
                        grouped_card("Sandbox & runners")
                            .child(card_divider())
                            .child(setting_row(
                                "Sandbox mode",
                                "Filesystem and network scope granted to agent steps.",
                                sandbox_picker,
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Preferred runner",
                                "Machine target used when starting runs for this project.",
                                runner_picker,
                            )),
                    )
            })),
    )
}

// --- Shared Project Settings Primitives -------------------------------------------------

fn section_heading(title: &'static str, subtitle: impl Into<String>) -> impl IntoElement {
    view()
        .gap(px(3.0))
        .child(
            text(title)
                .text_size(px(14.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            text(subtitle.into())
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
}

fn grouped_card(title: &str) -> View {
    view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view().px(px(16.0)).py(px(10.0)).child(
                text(title.to_owned())
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_secondary()),
            ),
        )
}

fn card_divider() -> impl IntoElement {
    view().h(px(1.0)).w_full().bg(palette::divider_color())
}

fn form_row_label(label: &'static str) -> impl IntoElement {
    view().w(px(140.0)).flex_shrink_0().child(
        text(label)
            .text_size(px(13.0))
            .text_color(palette::text_muted()),
    )
}

fn static_row(label: &'static str, value: String, monospace: bool) -> impl IntoElement {
    let mut val = text(value)
        .text_size(px(12.5))
        .text_color(palette::text_primary());
    if monospace {
        val = val.app_class("font-mono");
    }
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(11.0))
        .child(form_row_label(label))
        .child(view().flex_1().min_w(px(0.0)).child(val))
}

fn setting_row(
    title: impl Into<String>,
    subtitle: impl Into<String>,
    control: impl IntoElement,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .px(px(16.0))
        .py(px(12.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(title.into())
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(subtitle.into())
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(control)
}

fn toggle_row(
    title: impl Into<String>,
    subtitle: impl Into<String>,
    checked: bool,
    on_toggle: impl Fn() + 'static,
) -> impl IntoElement {
    let label = title.into();
    let a11y_label = label.clone();
    setting_row(
        label,
        subtitle,
        macos_switch(a11y_label, checked, on_toggle),
    )
}

fn macos_switch(label: String, checked: bool, on_toggle: impl Fn() + 'static) -> impl IntoElement {
    button()
        .accessibility_label(label)
        .on_press(on_toggle)
        .w(px(38.0))
        .h(px(22.0))
        .p(px(2.0))
        .rounded_full()
        .flex_row()
        .items_center()
        .justify_start()
        .bg(if checked {
            palette::nav_active_bg()
        } else {
            palette::switch_off_bg()
        })
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .w(px(18.0))
                .h(px(18.0))
                .rounded_full()
                .bg(palette::nav_active_fg())
                .ml(if checked { px(16.0) } else { px(0.0) }),
        )
}

fn macos_checkbox(
    test_id: &'static str,
    label: &'static str,
    checked: bool,
    on_toggle: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .on_press(on_toggle)
        .w(px(17.0))
        .h(px(17.0))
        .rounded(px(4.5))
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
            palette::card_inset_bg()
        })
        .active(|s| s.opacity(0.8))
        .child(when(
            move || checked,
            || icons::check(palette::nav_active_fg()),
        ))
}

fn checkbox_line(
    test_id: &'static str,
    label: &'static str,
    checked: bool,
    on_toggle: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .on_press(on_toggle)
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .w(px(17.0))
                .h(px(17.0))
                .rounded(px(4.5))
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
                    palette::card_inset_bg()
                })
                .child(when(
                    move || checked,
                    || icons::check(palette::nav_active_fg()),
                )),
        )
        .child(
            text(label)
                .text_size(px(13.0))
                .text_color(palette::text_primary()),
        )
}

fn segment_button(
    label: &'static str,
    active: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .px(px(11.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .bg(if active {
            palette::control_bg()
        } else {
            AdaptiveColor::TRANSPARENT
        })
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if active {
                    palette::text_primary()
                } else {
                    palette::text_muted()
                }),
        )
}

fn pill_choice(
    label: impl Into<String>,
    active: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .on_press(on_press)
        .px(px(10.0))
        .py(px(5.5))
        .rounded(px(7.0))
        .border_1()
        .border_color(if active {
            palette::card_selected_border()
        } else {
            palette::border_color()
        })
        .bg(if active {
            palette::card_selected_bg()
        } else {
            palette::control_bg()
        })
        .active(|s| s.opacity(0.85))
        .child(
            text(label.into())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
}

fn stepper_control(
    value_label: String,
    dec_label: &'static str,
    inc_label: &'static str,
    on_dec: impl Fn() + 'static,
    on_inc: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .h(px(28.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .overflow_hidden()
        .child(
            button()
                .accessibility_label(dec_label)
                .on_press(on_dec)
                .w(px(26.0))
                .h_full()
                .items_center()
                .justify_center()
                .border_r_1()
                .border_color(palette::divider_color())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text("−")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            view()
                .min_w(px(62.0))
                .px(px(10.0))
                .items_center()
                .justify_center()
                .child(
                    text(value_label)
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .tabular_nums()
                        .text_color(palette::text_primary()),
                ),
        )
        .child(
            button()
                .accessibility_label(inc_label)
                .on_press(on_inc)
                .w(px(26.0))
                .h_full()
                .items_center()
                .justify_center()
                .border_l_1()
                .border_color(palette::divider_color())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text("+")
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
}

fn secondary_button(
    test_id: &'static str,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .on_press(on_press)
        .px(px(12.0))
        .py(px(5.5))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
}

fn primary_button(
    test_id: &'static str,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .on_press(on_press)
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(7.0))
        .bg(palette::nav_active_bg())
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::nav_active_fg()),
        )
}

fn settings_messages_strip() -> impl IntoElement {
    view()
        .gap(px(4.0))
        .child(when(
            || store::project_settings_error().is_some(),
            || {
                text(move || store::project_settings_error().unwrap_or_default())
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::destructive_text())
            },
        ))
        .child(when(
            || store::project_settings_feedback().is_some(),
            || {
                text(move || store::project_settings_feedback().unwrap_or_default())
                    .text_size(px(12.0))
                    .text_color(palette::text_muted())
            },
        ))
}
