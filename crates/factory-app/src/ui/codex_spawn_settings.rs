//! Collapsible `<details>`-style Preferences editor for all custom Codex CLI spawn settings
//! (CLI launch flags, `-c` `config.toml` overrides, environment variables, and raw overrides),
//! with global filtering that hides empty groups and non-matching settings.

use std::collections::BTreeSet;

use factory_core::{
    CODEX_DISABLE_UPDATE_CHECK_KEY, CODEX_DISABLE_UPDATE_CHECK_VALUE, CODEX_ENV_PRESETS,
    CodexEnvPreset, CodexSettingCategory, CodexSettingKind, CodexSettingSpec, CodexSettingValue,
    CodexSpawnSettings, codex_setting_specs_for_category,
};
use nsh_base::{button, input};
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{icons, palette},
};

/// Persistent UI state for the Codex Spawn Settings card, owned outside `dynamic(...)`
/// blocks in `providers_tab()` so edits never drop search text, open editors, or collapse state.
#[derive(Clone, Copy)]
pub(crate) struct CodexSpawnUiState {
    pub(crate) expanded_categories: Signal<BTreeSet<CodexSettingCategory>>,
    pub(crate) filter_collapsed: Signal<BTreeSet<CodexSettingCategory>>,
    pub(crate) search: Signal<String>,
    pub(crate) modified_only: Signal<bool>,
    pub(crate) preview_open: Signal<bool>,
    pub(crate) editing_spec: Signal<Option<&'static str>>,
    pub(crate) draft_value: Signal<String>,
    pub(crate) env_editor_open: Signal<bool>,
    pub(crate) draft_env_key: Signal<String>,
    pub(crate) draft_env_value: Signal<String>,
    pub(crate) editing_extra_args: Signal<bool>,
    pub(crate) draft_extra_args: Signal<String>,
    pub(crate) editing_extra_config: Signal<bool>,
    pub(crate) draft_extra_config: Signal<String>,
}

impl CodexSpawnUiState {
    pub(crate) fn new() -> Self {
        Self {
            expanded_categories: signal(BTreeSet::new()),
            filter_collapsed: signal(BTreeSet::new()),
            search: signal(String::new()),
            modified_only: signal(false),
            preview_open: signal(false),
            editing_spec: signal(None),
            draft_value: signal(String::new()),
            env_editor_open: signal(false),
            draft_env_key: signal(String::new()),
            draft_env_value: signal(String::new()),
            editing_extra_args: signal(false),
            draft_extra_args: signal(String::new()),
            editing_extra_config: signal(false),
            draft_extra_config: signal(String::new()),
        }
    }

    fn is_section_expanded(&self, category: CodexSettingCategory, filtering: bool) -> bool {
        if filtering {
            !self.filter_collapsed.get().contains(&category)
        } else {
            self.expanded_categories.get().contains(&category)
        }
    }

    fn toggle_section(&self, category: CodexSettingCategory, filtering: bool) {
        if filtering {
            self.filter_collapsed.update(|set| {
                if !set.remove(&category) {
                    set.insert(category);
                }
            });
        } else {
            self.expanded_categories.update(|set| {
                if !set.remove(&category) {
                    set.insert(category);
                }
            });
        }
    }
}

/// Complete `Spawn Settings` card rendered inside `Preferences › Providers › Codex`.
pub(crate) fn codex_spawn_settings_card(ui: CodexSpawnUiState) -> impl IntoElement {
    let settings = store::codex_spawn_settings();
    let total_overrides = settings.override_count();

    view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(card_header(ui, total_overrides))
        .child(divider())
        .child(global_filter_bar(ui))
        .child(when(
            move || ui.preview_open.get(),
            move || {
                view()
                    .child(divider())
                    .child(spawn_command_preview_section())
            },
        ))
        .child(when(
            || store::preferences_error().is_some(),
            || {
                view().child(divider()).child(
                    view()
                        .px(px(16.0))
                        .py(px(8.0))
                        .bg(palette::status_error_bg())
                        .child(
                            text(move || store::preferences_error().unwrap_or_default())
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::destructive_text()),
                        ),
                )
            },
        ))
        .child(divider())
        .child(dynamic(move || {
            let current_settings = store::codex_spawn_settings();
            let query = ui.search.get();
            let modified_only = ui.modified_only.get();
            collapsible_groups_body(ui, &current_settings, &query, modified_only)
        }))
}

fn card_header(ui: CodexSpawnUiState, total_overrides: usize) -> impl IntoElement {
    let count_badge = if total_overrides == 0 {
        "All defaults".to_owned()
    } else if total_overrides == 1 {
        "1 override".to_owned()
    } else {
        format!("{total_overrides} overrides")
    };

    view()
        .px(px(16.0))
        .py(px(11.0))
        .gap(px(6.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text("Spawn Settings")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_secondary()),
                        )
                        .child(
                            view()
                                .px(px(6.5))
                                .py(px(1.5))
                                .rounded(px(5.0))
                                .border_1()
                                .border_color(if total_overrides > 0 {
                                    palette::card_selected_border()
                                } else {
                                    palette::border_color()
                                })
                                .bg(if total_overrides > 0 {
                                    palette::card_selected_bg()
                                } else {
                                    palette::card_inset_bg()
                                })
                                .child(
                                    text(count_badge)
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if total_overrides > 0 {
                                            palette::link_blue()
                                        } else {
                                            palette::text_muted()
                                        }),
                                ),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            button()
                                .test_id("prefs-codex-spawn-preview-toggle")
                                .on_press(move || ui.preview_open.update(|open| *open = !*open))
                                .px(px(9.0))
                                .py(px(3.5))
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(move || {
                                    if ui.preview_open.get() {
                                        palette::card_selected_border()
                                    } else {
                                        palette::border_color()
                                    }
                                })
                                .bg(move || {
                                    if ui.preview_open.get() {
                                        palette::card_selected_bg()
                                    } else {
                                        palette::control_bg()
                                    }
                                })
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text(move || {
                                        if ui.preview_open.get() {
                                            "Hide Preview"
                                        } else {
                                            "Preview Command"
                                        }
                                    })
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("prefs-codex-spawn-reset-all")
                                .disabled(total_overrides == 0)
                                .on_press(move || {
                                    ui.editing_spec.set(None);
                                    ui.env_editor_open.set(false);
                                    ui.editing_extra_args.set(false);
                                    ui.editing_extra_config.set(false);
                                    store::reset_all_codex_spawn_settings();
                                })
                                .px(px(9.0))
                                .py(px(3.5))
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .active(|s| s.opacity(0.8))
                                .disabled_style(|s| s.opacity(0.45))
                                .child(
                                    text("Reset All")
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::destructive_text()),
                                ),
                        ),
                ),
        )
        .child(
            text(
                "Custom CLI flags, config.toml overrides (-c key=value), and environment variables passed whenever Bruk spawns Codex. Unset options inherit your ~/.codex/config.toml defaults.",
            )
            .text_size(px(11.5))
            .text_color(palette::text_muted()),
        )
}

fn global_filter_bar(ui: CodexSpawnUiState) -> impl IntoElement {
    view()
        .px(px(16.0))
        .py(px(10.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            input()
                .test_id("prefs-codex-spawn-search")
                .value(move || ui.search.get())
                .on_change(move |v: &str| {
                    ui.search.set(v.to_owned());
                    ui.filter_collapsed.set(BTreeSet::new());
                })
                .placeholder("Filter all Codex flags, config.toml keys, and env vars...")
                .flex_1()
                .px(px(9.0))
                .py(px(5.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .rounded(px(7.0))
                .text_color(palette::text_primary()),
        )
        .child(
            button()
                .test_id("prefs-codex-spawn-modified-only")
                .on_press(move || {
                    ui.modified_only.update(|m| *m = !*m);
                    ui.filter_collapsed.set(BTreeSet::new());
                })
                .px(px(9.5))
                .py(px(5.5))
                .rounded(px(7.0))
                .border_1()
                .border_color(move || {
                    if ui.modified_only.get() {
                        palette::card_selected_border()
                    } else {
                        palette::border_color()
                    }
                })
                .bg(move || {
                    if ui.modified_only.get() {
                        palette::card_selected_bg()
                    } else {
                        palette::control_bg()
                    }
                })
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Modified only")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || {
                            if ui.modified_only.get() {
                                palette::link_blue()
                            } else {
                                palette::text_secondary()
                            }
                        }),
                ),
        )
        .child(when(
            move || ui.search.get().trim().is_empty() && !ui.modified_only.get(),
            move || {
                button()
                    .test_id("prefs-codex-spawn-expand-toggle")
                    .on_press(move || {
                        let all_expanded = ui.expanded_categories.get_untracked().len()
                            == CodexSettingCategory::ALL.len();
                        if all_expanded {
                            ui.expanded_categories.set(BTreeSet::new());
                        } else {
                            let all: BTreeSet<CodexSettingCategory> =
                                CodexSettingCategory::ALL.iter().copied().collect();
                            ui.expanded_categories.set(all);
                        }
                    })
                    .px(px(9.5))
                    .py(px(5.5))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::control_bg())
                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                    .active(|s| s.opacity(0.8))
                    .child(
                        text(move || {
                            if ui.expanded_categories.get().len() == CodexSettingCategory::ALL.len()
                            {
                                "Collapse All"
                            } else {
                                "Expand All"
                            }
                        })
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                    )
            },
        ))
}

fn spawn_command_preview_section() -> impl IntoElement {
    let preview = store::codex_spawn_preview_plan();
    let mut box_view = view()
        .px(px(16.0))
        .py(px(11.0))
        .gap(px(8.0))
        .bg(palette::card_inset_bg());

    match preview {
        Ok(plan) => {
            box_view = box_view
                .child(
                    text("Resolved spawn invocation")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_secondary()),
                )
                .child(
                    view()
                        .p(px(9.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .child(
                            text(plan.preview_command_line())
                                .test_id("prefs-codex-spawn-preview-cmd")
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .text_color(palette::text_primary()),
                        ),
                );
            if let Some(pretty_overrides) = plan.config_overrides_pretty() {
                box_view = box_view
                    .child(
                        text("Generated -c config.toml overrides")
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_secondary()),
                    )
                    .child(
                        view()
                            .p(px(9.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_bg())
                            .child(
                                text(pretty_overrides)
                                    .test_id("prefs-codex-spawn-preview-config")
                                    .app_class("font-mono")
                                    .text_size(px(11.5))
                                    .text_color(palette::text_primary()),
                            ),
                    );
            }
        }
        Err(err) => {
            box_view = box_view.child(
                text(format!("Could not build spawn command preview: {err}"))
                    .text_size(px(12.0))
                    .text_color(palette::destructive_text()),
            );
        }
    }

    box_view
}

struct EnvRawFilterMatches<'a> {
    show_mandatory_update_guard: bool,
    matching_env_vars: Vec<(&'a String, &'a String)>,
    matching_presets: Vec<&'static CodexEnvPreset>,
    show_extra_args: bool,
    show_extra_config: bool,
    show_add_env_controls: bool,
}

impl EnvRawFilterMatches<'_> {
    fn match_count(&self, filtering: bool, modified_only: bool) -> usize {
        if !filtering {
            return self.matching_env_vars.len() + 2;
        }
        if modified_only {
            self.matching_env_vars.len()
                + usize::from(self.show_extra_args)
                + usize::from(self.show_extra_config)
        } else {
            usize::from(self.show_mandatory_update_guard)
                + self.matching_env_vars.len()
                + self.matching_presets.len()
                + usize::from(self.show_extra_args)
                + usize::from(self.show_extra_config)
        }
    }

    fn has_any_match(&self, filtering: bool, modified_only: bool) -> bool {
        if !filtering {
            return true;
        }
        self.match_count(filtering, modified_only) > 0
    }
}

fn filter_env_raw<'a>(
    settings: &'a CodexSpawnSettings,
    query: &str,
    modified_only: bool,
) -> EnvRawFilterMatches<'a> {
    let trimmed = query.trim();
    let needle = trimmed.to_ascii_lowercase();
    let searching = !needle.is_empty();

    let show_mandatory_update_guard = !modified_only
        && (!searching
            || CODEX_DISABLE_UPDATE_CHECK_KEY
                .to_ascii_lowercase()
                .contains(&needle)
            || "always enforced by bruk so runs cannot check for or prompt codex updates"
                .contains(&needle)
            || "update".contains(&needle)
            || "config".contains(&needle));

    let matching_env_vars: Vec<(&String, &String)> = settings
        .env_vars
        .iter()
        .filter(|(k, v)| {
            !searching
                || k.to_ascii_lowercase().contains(&needle)
                || v.to_ascii_lowercase().contains(&needle)
                || "environment".contains(&needle)
                || "env".contains(&needle)
        })
        .collect();

    let matching_presets: Vec<&'static CodexEnvPreset> = if modified_only {
        Vec::new()
    } else {
        CODEX_ENV_PRESETS
            .iter()
            .filter(|preset| preset.matches_query(trimmed))
            .collect()
    };

    let show_extra_args = (!modified_only || !settings.extra_cli_args.trim().is_empty())
        && (!searching
            || "extra cli arguments".contains(&needle)
            || "additional shell-quoted cli arguments".contains(&needle)
            || settings
                .extra_cli_args
                .to_ascii_lowercase()
                .contains(&needle));

    let show_extra_config = (!modified_only || !settings.extra_config_overrides.trim().is_empty())
        && (!searching
            || "extra -c config.toml overrides".contains(&needle)
            || "raw key = value toml overrides".contains(&needle)
            || settings
                .extra_config_overrides
                .to_ascii_lowercase()
                .contains(&needle));

    let show_add_env_controls = !modified_only
        && (!searching
            || show_mandatory_update_guard
            || !matching_env_vars.is_empty()
            || !matching_presets.is_empty());

    EnvRawFilterMatches {
        show_mandatory_update_guard,
        matching_env_vars,
        matching_presets,
        show_extra_args,
        show_extra_config,
        show_add_env_controls,
    }
}

fn collapsible_groups_body(
    ui: CodexSpawnUiState,
    settings: &CodexSpawnSettings,
    query: &str,
    modified_only: bool,
) -> impl IntoElement + use<> {
    let searching = !query.trim().is_empty();
    let filtering = searching || modified_only;

    let mut sections: Vec<AnyElement> = Vec::new();
    let mut total_visible_matches = 0usize;
    let mut visible_group_count = 0usize;

    for category in CodexSettingCategory::ALL {
        let cat = *category;
        let override_count = settings.override_count_in_category(cat);

        if cat == CodexSettingCategory::EnvironmentAndRaw {
            let env_matches = filter_env_raw(settings, query, modified_only);
            if !env_matches.has_any_match(filtering, modified_only) {
                continue;
            }
            visible_group_count += 1;
            let count = env_matches.match_count(filtering, modified_only);
            total_visible_matches += count;

            let expanded = ui.is_section_expanded(cat, filtering);
            let count_label = if modified_only && !searching {
                format!("{count} modified")
            } else if searching {
                format!("{count} matching")
            } else {
                format!("{} presets + raw", CODEX_ENV_PRESETS.len())
            };

            let body = if expanded {
                Some(environment_and_raw_filtered_body(ui, settings, &env_matches).into_element())
            } else {
                None
            };

            sections.push(
                details_section(
                    ui,
                    cat,
                    filtering,
                    expanded,
                    override_count,
                    count_label,
                    body,
                )
                .into_element(),
            );
            continue;
        }

        let all_specs = codex_setting_specs_for_category(cat);
        let total_in_group = all_specs.len();
        let matching_specs: Vec<&'static CodexSettingSpec> = all_specs
            .into_iter()
            .filter(|spec| {
                let text_ok = !searching || spec.matches_query(query);
                let mod_ok = !modified_only || settings.get(spec.id).is_some();
                text_ok && mod_ok
            })
            .collect();

        if matching_specs.is_empty() {
            continue;
        }

        visible_group_count += 1;
        total_visible_matches += matching_specs.len();

        let expanded = ui.is_section_expanded(cat, filtering);
        let count_label = if searching {
            format!("{} of {total_in_group}", matching_specs.len())
        } else if modified_only {
            format!("{} of {total_in_group} modified", matching_specs.len())
        } else {
            format!("{total_in_group} settings")
        };

        let body = if expanded {
            let mut rows = view();
            for (idx, spec) in matching_specs.into_iter().enumerate() {
                if idx > 0 {
                    rows = rows.child(divider());
                }
                let current = settings.get(spec.id).cloned();
                rows = rows.child(setting_spec_row(ui, spec, current));
            }
            Some(rows.into_element())
        } else {
            None
        };

        sections.push(
            details_section(
                ui,
                cat,
                filtering,
                expanded,
                override_count,
                count_label,
                body,
            )
            .into_element(),
        );
    }

    if sections.is_empty() {
        let empty_msg = if modified_only && !searching {
            "No custom Codex spawn overrides configured yet. Everything currently inherits your ~/.codex/config.toml defaults."
                .to_owned()
        } else {
            format!("No Codex settings match '{}'.", query.trim())
        };
        return view().px(px(16.0)).py(px(16.0)).child(
            text(empty_msg)
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    }

    let mut container = view().p(px(12.0)).gap(px(8.0)).bg(palette::card_inset_bg());

    if filtering {
        let summary_text = if modified_only && !searching {
            format!(
                "Showing {total_visible_matches} active override(s) across {visible_group_count} group(s)"
            )
        } else {
            format!(
                "Showing {total_visible_matches} matching setting(s) across {visible_group_count} group(s)"
            )
        };
        container = container.child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(4.0))
                .child(
                    text(summary_text)
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .child(
                    button()
                        .test_id("prefs-codex-spawn-clear-filter")
                        .on_press(move || {
                            ui.search.set(String::new());
                            ui.modified_only.set(false);
                            ui.filter_collapsed.set(BTreeSet::new());
                        })
                        .px(px(7.0))
                        .py(px(2.5))
                        .rounded(px(5.0))
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Clear filter")
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::link_blue()),
                        ),
                ),
        );
    }

    for section in sections {
        container = container.child(section);
    }

    container
}

/// Renders one `<details>`/`<summary>`-style collapsible group panel.
fn details_section(
    ui: CodexSpawnUiState,
    category: CodexSettingCategory,
    filtering: bool,
    expanded: bool,
    override_count: usize,
    count_label: String,
    body: Option<AnyElement>,
) -> impl IntoElement {
    let section_test_id = format!("prefs-codex-spawn-section-{}", category.slug());
    let chevron: AnyElement = if expanded {
        icons::chevron_down(palette::text_secondary()).into_element()
    } else {
        icons::chevron_right(palette::text_secondary()).into_element()
    };

    let mut right_meta = view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .gap(px(6.0));

    if override_count > 0 {
        let custom_label = if override_count == 1 {
            "1 custom".to_owned()
        } else {
            format!("{override_count} custom")
        };
        right_meta = right_meta.child(
            view()
                .px(px(6.0))
                .py(px(1.5))
                .rounded(px(4.5))
                .border_1()
                .border_color(palette::card_selected_border())
                .bg(palette::card_selected_bg())
                .child(
                    text(custom_label)
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::link_blue()),
                ),
        );
    }

    right_meta = right_meta.child(
        text(count_label)
            .text_size(px(11.0))
            .text_color(palette::text_muted()),
    );

    let summary_bar = button()
        .test_id(&section_test_id)
        .accessibility_label(format!(
            "{} section ({})",
            category.label(),
            if expanded { "expanded" } else { "collapsed" }
        ))
        .on_press(move || ui.toggle_section(category, filtering))
        .w_full()
        .px(px(12.0))
        .py(px(9.5))
        .bg(if expanded {
            palette::subtle_surface_bg()
        } else {
            palette::card_bg()
        })
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.85))
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
                        .gap(px(8.0))
                        .child(chevron)
                        .child(
                            view()
                                .flex_1()
                                .min_w(px(0.0))
                                .gap(px(1.5))
                                .child(
                                    text(category.label())
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::text_primary()),
                                )
                                .child(
                                    text(category.description())
                                        .text_size(px(11.0))
                                        .text_color(palette::text_muted()),
                                ),
                        ),
                )
                .child(right_meta),
        );

    let mut panel = view()
        .rounded(px(8.0))
        .border_1()
        .border_color(if expanded {
            palette::border_color()
        } else {
            palette::divider_color()
        })
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(summary_bar);

    if let Some(content) = body {
        panel = panel.child(divider()).child(content);
    }

    panel
}

fn setting_spec_row(
    ui: CodexSpawnUiState,
    spec: &'static CodexSettingSpec,
    current: Option<CodexSettingValue>,
) -> impl IntoElement {
    let is_modified = current.is_some();
    let slug = slugify_id(spec.id);
    let is_editing = ui.editing_spec.get() == Some(spec.id);

    let mut title_row = view()
        .flex_row()
        .items_center()
        .flex_wrap()
        .gap(px(6.0))
        .child(
            text(spec.label)
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .px(px(5.5))
                .py(px(1.0))
                .rounded(px(4.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .child(
                    text(spec.display_key)
                        .app_class("font-mono")
                        .text_size(px(10.5))
                        .text_color(palette::text_secondary()),
                ),
        );

    if is_modified {
        title_row = title_row.child(
            view()
                .px(px(5.5))
                .py(px(1.0))
                .rounded(px(4.5))
                .border_1()
                .border_color(palette::card_selected_border())
                .bg(palette::card_selected_bg())
                .child(
                    text("Custom")
                        .text_size(px(10.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::link_blue()),
                ),
        );
    }

    let description_text = match spec.default_hint_label() {
        Some(hint) => format!("{} ({hint})", spec.description),
        None => spec.description.to_owned(),
    };

    let right_control: AnyElement = match spec.kind {
        CodexSettingKind::Bool { .. } => {
            let current_bool = match current {
                Some(CodexSettingValue::Bool(b)) => Some(b),
                _ => None,
            };
            bool_tri_state_control(spec.id, &slug, current_bool).into_element()
        }
        CodexSettingKind::Choice {
            options,
            allow_custom,
            ..
        } if !allow_custom && options.len() <= 3 => {
            let current_choice = match current {
                Some(CodexSettingValue::String(ref s)) => Some(s.as_str()),
                _ => None,
            };
            compact_choice_control(spec.id, &slug, options, current_choice).into_element()
        }
        CodexSettingKind::Choice { .. } => {
            let current_display = current
                .as_ref()
                .map(CodexSettingValue::summary)
                .unwrap_or_else(|| "Default".to_owned());
            let edit_seed = current
                .as_ref()
                .map(CodexSettingValue::to_edit_text)
                .unwrap_or_default();
            editable_summary_controls(
                ui,
                spec.id,
                &slug,
                current_display,
                edit_seed,
                is_modified,
                is_editing,
                "Choose...",
            )
            .into_element()
        }
        CodexSettingKind::String { .. }
        | CodexSettingKind::Integer { .. }
        | CodexSettingKind::Number { .. }
        | CodexSettingKind::StringList { .. }
        | CodexSettingKind::Json { .. } => {
            let current_display = current
                .as_ref()
                .map(CodexSettingValue::summary)
                .unwrap_or_else(|| "Default".to_owned());
            let edit_seed = current
                .as_ref()
                .map(CodexSettingValue::to_edit_text)
                .unwrap_or_default();
            editable_summary_controls(
                ui,
                spec.id,
                &slug,
                current_display,
                edit_seed,
                is_modified,
                is_editing,
                if is_modified { "Edit..." } else { "Set..." },
            )
            .into_element()
        }
    };

    let mut row = view().px(px(14.0)).py(px(10.0)).gap(px(8.0)).child(
        view()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(2.5))
                    .child(title_row)
                    .child(
                        text(description_text)
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                    ),
            )
            .child(right_control),
    );

    if is_editing {
        row = row.child(inline_setting_editor(ui, spec, &slug, current));
    }

    row
}

fn bool_tri_state_control(
    spec_id: &'static str,
    slug: &str,
    current: Option<bool>,
) -> impl IntoElement {
    view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .p(px(2.0))
        .rounded(px(7.0))
        .bg(palette::segmented_track_bg())
        .child(tri_segment_btn(
            &format!("prefs-codex-spawn-{slug}-default"),
            "Default",
            current.is_none(),
            move || store::set_codex_spawn_bool(spec_id, None),
        ))
        .child(tri_segment_btn(
            &format!("prefs-codex-spawn-{slug}-on"),
            "On",
            current == Some(true),
            move || store::set_codex_spawn_bool(spec_id, Some(true)),
        ))
        .child(tri_segment_btn(
            &format!("prefs-codex-spawn-{slug}-off"),
            "Off",
            current == Some(false),
            move || store::set_codex_spawn_bool(spec_id, Some(false)),
        ))
}

fn compact_choice_control(
    spec_id: &'static str,
    slug: &str,
    options: &'static [&'static str],
    current: Option<&str>,
) -> impl IntoElement {
    let mut seg = view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .p(px(2.0))
        .rounded(px(7.0))
        .bg(palette::segmented_track_bg())
        .child(tri_segment_btn(
            &format!("prefs-codex-spawn-{slug}-default"),
            "Default",
            current.is_none(),
            move || store::set_codex_spawn_choice(spec_id, None),
        ));

    for opt in options {
        let opt_val = *opt;
        let opt_slug = slugify_id(opt_val);
        seg = seg.child(tri_segment_btn(
            &format!("prefs-codex-spawn-{slug}-{opt_slug}"),
            opt_val,
            current == Some(opt_val),
            move || store::set_codex_spawn_choice(spec_id, Some(opt_val)),
        ));
    }

    seg
}

fn tri_segment_btn(
    test_id: &str,
    label: &str,
    active: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .on_press(on_press)
        .px(px(8.0))
        .py(px(3.5))
        .rounded(px(5.5))
        .bg(if active {
            palette::control_bg()
        } else {
            AdaptiveColor::TRANSPARENT
        })
        .active(|s| s.opacity(0.8))
        .child(
            text(label.to_owned())
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if active {
                    palette::text_primary()
                } else {
                    palette::text_muted()
                }),
        )
}

#[allow(clippy::too_many_arguments)]
fn editable_summary_controls(
    ui: CodexSpawnUiState,
    spec_id: &'static str,
    slug: &str,
    current_display: String,
    edit_seed: String,
    is_modified: bool,
    is_editing: bool,
    action_label: &'static str,
) -> impl IntoElement {
    let mut controls = view()
        .flex_shrink_0()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(
            text(current_display)
                .app_class("font-mono")
                .text_size(px(11.5))
                .text_color(if is_modified {
                    palette::text_primary()
                } else {
                    palette::text_muted()
                }),
        )
        .child(
            button()
                .test_id(&format!("prefs-codex-spawn-{slug}-edit"))
                .on_press(move || {
                    store::clear_preferences_messages();
                    if is_editing {
                        ui.editing_spec.set(None);
                    } else {
                        ui.draft_value.set(edit_seed.clone());
                        ui.editing_spec.set(Some(spec_id));
                    }
                })
                .px(px(9.0))
                .py(px(4.0))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text(if is_editing { "Close" } else { action_label })
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        );

    if is_modified {
        controls = controls.child(
            button()
                .test_id(&format!("prefs-codex-spawn-{slug}-reset"))
                .on_press(move || {
                    if ui.editing_spec.get_untracked() == Some(spec_id) {
                        ui.editing_spec.set(None);
                    }
                    store::clear_codex_spawn_setting(spec_id);
                })
                .px(px(7.5))
                .py(px(4.0))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Reset")
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    controls
}

fn inline_setting_editor(
    ui: CodexSpawnUiState,
    spec: &'static CodexSettingSpec,
    slug: &str,
    current: Option<CodexSettingValue>,
) -> impl IntoElement {
    match spec.kind {
        CodexSettingKind::Choice {
            options,
            allow_custom,
            ..
        } => {
            let selected_str = match current {
                Some(CodexSettingValue::String(ref s)) => Some(s.as_str()),
                _ => None,
            };
            let mut chips = view()
                .flex_row()
                .flex_wrap()
                .gap(px(6.0))
                .child(choice_chip(
                    &format!("prefs-codex-spawn-{slug}-opt-default"),
                    "Default",
                    selected_str.is_none(),
                    move || {
                        store::set_codex_spawn_choice(spec.id, None);
                        ui.editing_spec.set(None);
                    },
                ));
            for opt in options {
                let opt_val = *opt;
                let opt_slug = slugify_id(opt_val);
                chips = chips.child(choice_chip(
                    &format!("prefs-codex-spawn-{slug}-opt-{opt_slug}"),
                    opt_val,
                    selected_str == Some(opt_val),
                    move || {
                        store::set_codex_spawn_choice(spec.id, Some(opt_val));
                        ui.editing_spec.set(None);
                    },
                ));
            }

            let mut container = view()
                .p(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .gap(px(8.0))
                .child(chips);

            if allow_custom {
                container = container.child(inline_text_save_row(
                    ui,
                    spec.id,
                    slug,
                    "Or enter custom value...",
                ));
            }
            container
        }
        CodexSettingKind::String { placeholder, .. }
        | CodexSettingKind::Integer { placeholder, .. }
        | CodexSettingKind::Number { placeholder, .. }
        | CodexSettingKind::StringList { placeholder, .. }
        | CodexSettingKind::Json { placeholder, .. } => {
            let helper = match spec.kind {
                CodexSettingKind::StringList { .. } => {
                    Some("Enter comma-separated values (e.g. item1, item2).")
                }
                CodexSettingKind::Json {
                    expect_array: true, ..
                } => Some("Enter a valid JSON array ([ ... ]); serialized to TOML for -c."),
                CodexSettingKind::Json {
                    expect_array: false,
                    ..
                } => Some(
                    "Enter a valid JSON object ({ ... }); serialized to a TOML inline table for -c.",
                ),
                CodexSettingKind::String {
                    allow_empty: true, ..
                } => Some(
                    "Save blank text to explicitly pass an empty value, or Reset to inherit CLI default.",
                ),
                _ => None,
            };

            let mut container = view()
                .p(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .gap(px(7.0));

            if let Some(hint) = helper {
                container = container.child(
                    text(hint)
                        .text_size(px(11.0))
                        .text_color(palette::text_muted()),
                );
            }

            container.child(inline_text_save_row(ui, spec.id, slug, placeholder))
        }
        CodexSettingKind::Bool { .. } => view(),
    }
}

fn inline_text_save_row(
    ui: CodexSpawnUiState,
    spec_id: &'static str,
    slug: &str,
    placeholder: &'static str,
) -> impl IntoElement {
    let save = move || {
        let raw = ui.draft_value.get_untracked();
        if store::set_codex_spawn_text(spec_id, &raw) {
            ui.editing_spec.set(None);
        }
    };
    let on_submit = save;

    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            input()
                .test_id(&format!("prefs-codex-spawn-{slug}-input"))
                .value(move || ui.draft_value.get())
                .on_change(move |v: &str| ui.draft_value.set(v.to_owned()))
                .on_submit(move |_: &str| on_submit())
                .placeholder(placeholder)
                .app_class("font-mono")
                .flex_1()
                .px(px(9.0))
                .py(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_bg())
                .rounded(px(6.5))
                .text_color(palette::text_primary()),
        )
        .child(
            button()
                .test_id(&format!("prefs-codex-spawn-{slug}-save"))
                .on_press(save)
                .px(px(11.0))
                .py(px(5.5))
                .rounded(px(6.5))
                .bg(palette::nav_active_bg())
                .active(|s| s.opacity(0.8))
                .child(
                    text("Save")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                ),
        )
        .child(
            button()
                .test_id(&format!("prefs-codex-spawn-{slug}-cancel"))
                .on_press(move || {
                    store::clear_preferences_messages();
                    ui.editing_spec.set(None);
                })
                .px(px(10.0))
                .py(px(5.5))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text("Cancel")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        )
}

fn choice_chip(
    test_id: &str,
    label: &str,
    selected: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .on_press(on_press)
        .px(px(9.0))
        .py(px(4.5))
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
            palette::card_bg()
        })
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(
            text(label.to_owned())
                .app_class("font-mono")
                .text_size(px(11.5))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    palette::link_blue()
                } else {
                    palette::text_primary()
                }),
        )
}

// --- Environment & Raw Overrides Section ------------------------------------------------

fn environment_and_raw_filtered_body(
    ui: CodexSpawnUiState,
    settings: &CodexSpawnSettings,
    matches: &EnvRawFilterMatches<'_>,
) -> impl IntoElement {
    let mut section = view();
    let mut has_child = false;

    let show_env_block = matches.show_mandatory_update_guard
        || !matches.matching_env_vars.is_empty()
        || matches.show_add_env_controls;
    if show_env_block {
        section = section.child(env_vars_section(ui, matches));
        has_child = true;
    }

    if matches.show_extra_args {
        if has_child {
            section = section.child(divider());
        }
        section = section.child(extra_cli_args_row(ui, settings.extra_cli_args.clone()));
        has_child = true;
    }

    if matches.show_extra_config {
        if has_child {
            section = section.child(divider());
        }
        section = section.child(extra_config_overrides_row(
            ui,
            settings.extra_config_overrides.clone(),
        ));
        has_child = true;
    }

    if !matches.matching_presets.is_empty() {
        if has_child {
            section = section.child(divider());
        }
        section = section.child(env_presets_section(ui, settings, &matches.matching_presets));
    }

    section
}

fn env_vars_section(ui: CodexSpawnUiState, matches: &EnvRawFilterMatches<'_>) -> impl IntoElement {
    let is_editor_open = ui.env_editor_open.get();
    let mut header = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            view()
                .gap(px(2.0))
                .child(
                    text("Process Environment Variables")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("Applied to the spawned codex process environment before startup.")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        );

    if matches.show_add_env_controls {
        header = header.child(
            button()
                .test_id("prefs-codex-spawn-env-add-btn")
                .on_press(move || {
                    store::clear_preferences_messages();
                    if is_editor_open {
                        ui.env_editor_open.set(false);
                    } else {
                        ui.draft_env_key.set(String::new());
                        ui.draft_env_value.set(String::new());
                        ui.env_editor_open.set(true);
                    }
                })
                .px(px(9.5))
                .py(px(4.5))
                .rounded(px(6.5))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text(if is_editor_open {
                        "Close"
                    } else {
                        "Add Variable..."
                    })
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_primary()),
                ),
        );
    }

    let mut container = view().px(px(14.0)).py(px(11.0)).gap(px(9.0)).child(header);

    if matches.show_mandatory_update_guard {
        container = container.child(mandatory_guard_row(
            CODEX_DISABLE_UPDATE_CHECK_KEY,
            CODEX_DISABLE_UPDATE_CHECK_VALUE,
            "Always enforced by Bruk (-c check_for_update_on_startup=false) so runs cannot check for or prompt Codex updates.",
        ));
    }

    if matches.matching_env_vars.is_empty() {
        if matches.show_add_env_controls {
            container = container.child(
                text("No additional custom environment variables configured.")
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
            );
        }
    } else {
        let mut rows = view().gap(px(6.0));
        for (key, value) in &matches.matching_env_vars {
            rows = rows.child(env_var_row(ui, (*key).clone(), (*value).clone()));
        }
        container = container.child(rows);
    }

    if is_editor_open {
        let save_env = move || {
            let key = ui.draft_env_key.get_untracked();
            let val = ui.draft_env_value.get_untracked();
            if store::set_codex_spawn_env_var(&key, &val) {
                ui.draft_env_key.set(String::new());
                ui.draft_env_value.set(String::new());
                ui.env_editor_open.set(false);
            }
        };
        let on_submit = save_env;

        container = container.child(
            view()
                .p(px(10.0))
                .rounded(px(8.0))
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
                            input()
                                .test_id("prefs-codex-spawn-env-key-input")
                                .value(move || ui.draft_env_key.get())
                                .on_change(move |v: &str| ui.draft_env_key.set(v.to_owned()))
                                .placeholder("ENV_VAR_NAME")
                                .app_class("font-mono")
                                .flex_1()
                                .px(px(9.0))
                                .py(px(6.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .rounded(px(6.5))
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            input()
                                .test_id("prefs-codex-spawn-env-value-input")
                                .value(move || ui.draft_env_value.get())
                                .on_change(move |v: &str| ui.draft_env_value.set(v.to_owned()))
                                .on_submit(move |_: &str| on_submit())
                                .placeholder("value")
                                .app_class("font-mono")
                                .flex_1()
                                .px(px(9.0))
                                .py(px(6.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .rounded(px(6.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .gap(px(8.0))
                        .child(
                            button()
                                .test_id("prefs-codex-spawn-env-cancel")
                                .on_press(move || {
                                    store::clear_preferences_messages();
                                    ui.env_editor_open.set(false);
                                })
                                .px(px(10.0))
                                .py(px(5.0))
                                .rounded(px(6.5))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .child(
                                    text("Cancel")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("prefs-codex-spawn-env-save")
                                .on_press(save_env)
                                .px(px(11.0))
                                .py(px(5.0))
                                .rounded(px(6.5))
                                .bg(palette::nav_active_bg())
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text("Save Variable")
                                        .text_size(px(12.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::nav_active_fg()),
                                ),
                        ),
                ),
        );
    }

    container
}

fn mandatory_guard_row(key: &str, value: &str, note: &str) -> impl IntoElement {
    view()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .gap(px(3.0))
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
                            text(key.to_owned())
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text("=")
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                        )
                        .child(
                            text(value.to_owned())
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .text_color(palette::text_secondary()),
                        ),
                )
                .child(
                    view()
                        .px(px(6.5))
                        .py(px(1.5))
                        .rounded(px(5.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .child(
                            text("Always enforced")
                                .text_size(px(10.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_secondary()),
                        ),
                ),
        )
        .child(
            text(note.to_owned())
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
        )
}

fn env_var_row(ui: CodexSpawnUiState, key: String, value: String) -> impl IntoElement {
    let key_slug = slugify_id(&key);
    let key_for_edit = key.clone();
    let val_for_edit = value.clone();
    let key_for_remove = key.clone();

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .px(px(10.0))
        .py(px(7.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .flex_wrap()
                .gap(px(6.0))
                .child(
                    text(key)
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("=")
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .child(
                    text(value)
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .text_color(palette::text_secondary()),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    button()
                        .test_id(&format!("prefs-codex-spawn-env-edit-{key_slug}"))
                        .on_press(move || {
                            store::clear_preferences_messages();
                            ui.draft_env_key.set(key_for_edit.clone());
                            ui.draft_env_value.set(val_for_edit.clone());
                            ui.env_editor_open.set(true);
                        })
                        .px(px(8.0))
                        .py(px(3.5))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Edit")
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    button()
                        .test_id(&format!("prefs-codex-spawn-env-remove-{key_slug}"))
                        .on_press(move || store::remove_codex_spawn_env_var(&key_for_remove))
                        .px(px(8.0))
                        .py(px(3.5))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Remove")
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::destructive_text()),
                        ),
                ),
        )
}

fn env_presets_section(
    ui: CodexSpawnUiState,
    settings: &CodexSpawnSettings,
    presets: &[&'static CodexEnvPreset],
) -> impl IntoElement {
    let mut chips = view().flex_row().flex_wrap().gap(px(6.0));
    for preset in presets {
        let already_set = settings.env_vars.contains_key(preset.name);
        let slug = slugify_id(preset.name);
        let preset_name = preset.name;
        let preset_val = settings
            .env_vars
            .get(preset.name)
            .cloned()
            .unwrap_or_else(|| preset.suggested_value.to_owned());

        chips = chips.child(
            button()
                .test_id(&format!("prefs-codex-spawn-env-preset-{slug}"))
                .accessibility_label(format!("{}: {}", preset.name, preset.description))
                .on_press(move || {
                    store::clear_preferences_messages();
                    ui.draft_env_key.set(preset_name.to_owned());
                    ui.draft_env_value.set(preset_val.clone());
                    ui.env_editor_open.set(true);
                })
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(if already_set {
                    palette::card_selected_border()
                } else {
                    palette::border_color()
                })
                .bg(if already_set {
                    palette::card_selected_bg()
                } else {
                    palette::card_inset_bg()
                })
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text(preset.name)
                        .app_class("font-mono")
                        .text_size(px(11.0))
                        .text_color(if already_set {
                            palette::link_blue()
                        } else {
                            palette::text_secondary()
                        }),
                ),
        );
    }

    view()
        .px(px(14.0))
        .py(px(11.0))
        .gap(px(8.0))
        .child(
            view()
                .gap(px(2.0))
                .child(
                    text("Documented Codex CLI Environment Presets")
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("Click any preset to pre-fill the environment variable editor above.")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(chips)
}

fn extra_cli_args_row(ui: CodexSpawnUiState, current: String) -> impl IntoElement {
    let is_modified = !current.trim().is_empty();
    let is_editing = ui.editing_extra_args.get();
    let seed = current.clone();
    let summary = if is_modified {
        current
    } else {
        "None".to_owned()
    };

    let save = move || {
        let raw = ui.draft_extra_args.get_untracked();
        if store::set_codex_spawn_extra_cli_args(&raw) {
            ui.editing_extra_args.set(false);
        }
    };
    let on_submit = save;

    let mut row = view().px(px(14.0)).py(px(11.0)).gap(px(8.0)).child(
        view()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(
                view()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(2.0))
                    .child(
                        text("Extra CLI Arguments")
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_primary()),
                    )
                    .child(
                        text("Additional shell-quoted CLI arguments appended when spawning codex.")
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        text(summary)
                            .app_class("font-mono")
                            .text_size(px(11.5))
                            .text_color(if is_modified {
                                palette::text_primary()
                            } else {
                                palette::text_muted()
                            }),
                    )
                    .child(
                        button()
                            .test_id("prefs-codex-spawn-extra-args-edit")
                            .on_press(move || {
                                store::clear_preferences_messages();
                                if is_editing {
                                    ui.editing_extra_args.set(false);
                                } else {
                                    ui.draft_extra_args.set(seed.clone());
                                    ui.editing_extra_args.set(true);
                                }
                            })
                            .px(px(9.0))
                            .py(px(4.0))
                            .rounded(px(6.5))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::control_bg())
                            .hover(|s| s.bg(palette::subtle_surface_bg()))
                            .child(
                                text(if is_editing {
                                    "Close"
                                } else if is_modified {
                                    "Edit..."
                                } else {
                                    "Set..."
                                })
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                            ),
                    ),
            ),
    );

    if is_editing {
        row = row.child(
            view()
                .p(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    input()
                        .test_id("prefs-codex-spawn-extra-args-input")
                        .value(move || ui.draft_extra_args.get())
                        .on_change(move |v: &str| ui.draft_extra_args.set(v.to_owned()))
                        .on_submit(move |_: &str| on_submit())
                        .placeholder("e.g. --profile fast --ephemeral")
                        .app_class("font-mono")
                        .flex_1()
                        .px(px(9.0))
                        .py(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .rounded(px(6.5))
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .test_id("prefs-codex-spawn-extra-args-save")
                        .on_press(save)
                        .px(px(11.0))
                        .py(px(5.5))
                        .rounded(px(6.5))
                        .bg(palette::nav_active_bg())
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Save")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        );
    }

    row
}

fn extra_config_overrides_row(ui: CodexSpawnUiState, current: String) -> impl IntoElement {
    let is_modified = !current.trim().is_empty();
    let is_editing = ui.editing_extra_config.get();
    let seed = current.clone();
    let summary = if is_modified {
        "Configured overrides".to_owned()
    } else {
        "None".to_owned()
    };

    let save = move || {
        let raw = ui.draft_extra_config.get_untracked();
        if store::set_codex_spawn_extra_config_overrides(&raw) {
            ui.editing_extra_config.set(false);
        }
    };
    let on_submit = save;

    let mut row = view()
        .px(px(14.0))
        .py(px(11.0))
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(2.0))
                        .child(
                            text("Extra -c config.toml Overrides")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(
                                "Raw key = value TOML overrides (one per line) emitted via -c for experimental or future Codex config keys.",
                            )
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            text(summary)
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .text_color(if is_modified {
                                    palette::text_primary()
                                } else {
                                    palette::text_muted()
                                }),
                        )
                        .child(
                            button()
                                .test_id("prefs-codex-spawn-extra-config-edit")
                                .on_press(move || {
                                    store::clear_preferences_messages();
                                    if is_editing {
                                        ui.editing_extra_config.set(false);
                                    } else {
                                        ui.draft_extra_config.set(seed.clone());
                                        ui.editing_extra_config.set(true);
                                    }
                                })
                                .px(px(9.0))
                                .py(px(4.0))
                                .rounded(px(6.5))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::control_bg())
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .child(
                                    text(if is_editing {
                                        "Close"
                                    } else if is_modified {
                                        "Edit..."
                                    } else {
                                        "Set..."
                                    })
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_primary()),
                                ),
                        ),
                ),
        );

    if is_editing {
        row = row.child(
            view()
                .p(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    input()
                        .test_id("prefs-codex-spawn-extra-config-input")
                        .value(move || ui.draft_extra_config.get())
                        .on_change(move |v: &str| ui.draft_extra_config.set(v.to_owned()))
                        .on_submit(move |_: &str| on_submit())
                        .placeholder(r#"custom_option = true"#)
                        .app_class("font-mono")
                        .flex_1()
                        .px(px(9.0))
                        .py(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .rounded(px(6.5))
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .test_id("prefs-codex-spawn-extra-config-save")
                        .on_press(save)
                        .px(px(11.0))
                        .py(px(5.5))
                        .rounded(px(6.5))
                        .bg(palette::nav_active_bg())
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Save")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        );
    }

    row
}

fn divider() -> impl IntoElement {
    view().h(px(1.0)).w_full().bg(palette::divider_color())
}

fn slugify_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}
