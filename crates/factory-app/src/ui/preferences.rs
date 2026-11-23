//! Native macOS Preferences window (`⌘,`) with tabbed settings and provider account management.

use factory_core::{
    AppearanceMode, ClaudeModelOption, ClaudeQuotaReset, ClaudeUsageSummary, ClaudeUsageWindow,
    CliAccount, ConnectionTestState, CoreUpdatePolicy, DefaultLandingView, MachineAgentKind,
    MachineConnectionStatus, MachinePlatform, MachineRecord, OpenRouterKey, PreferencesTab,
    ProviderKind, SchemaCompatibility, format_reset_schedule_now, format_whole_or_cents,
    model_family_from_id,
};
use nsh_base::{button, input};
use nsh_core::{
    desktop::{TitleBarOptions, TitleBarStyle, WindowAttributes, WindowKind, WindowPosition},
    element::desktop::{current_window, window},
    file_dialog::OpenDialog,
    prelude::*,
};

use crate::{
    store::{self, GeneralFlag},
    ui::{
        claude_spawn_settings::{ClaudeSpawnUiState, claude_spawn_settings_card},
        codex_spawn_settings::{CodexSpawnUiState, codex_spawn_settings_card},
        icons, palette,
    },
};

/// Active inline editor inside the Providers detail pane.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderEditor {
    AddOpenRouterKey,
    AddCodexAccount,
    ReplaceKey(String),
    EditCap(String),
    EditCliPath(String),
    ChangeModel(String),
    RenameKey(String),
    ConfirmRemove(String),
}

/// Active inline editor or disclosure panel inside the Machines detail pane.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MachinePrefsEditor {
    AddMachine,
    ChangeUser(u64),
    EditJumpHost(u64),
    ShowHostKey(u64),
    ChangeSshKey(u64),
    ChangeCoreUpdates(u64),
    ConfirmAgentSignOut(u64, MachineAgentKind),
    ChangeSessionsAtOnce(u64),
    EditProjects(u64),
    ConfirmRemove(u64),
}

/// Declarative secondary macOS window hosting application Preferences.
pub(crate) fn preferences_window() -> impl IntoElement {
    let mut title_bar = TitleBarOptions::default();
    title_bar.style = TitleBarStyle::Hidden;
    title_bar.height = Some(px(38.0));

    let mut attrs = WindowAttributes::default();
    attrs.title_bar = Some(title_bar);
    attrs.resizable = Some(true);
    attrs.min_size = Some(Some(Size::new(px(760.0), px(620.0))));
    attrs.close_on_escape = Some(true);

    window()
        .id("preferences")
        .kind(WindowKind::Dialog)
        .parent("main")
        .default_size(Size::new(px(860.0), px(740.0)))
        .default_position(WindowPosition::ParentCenter)
        .title(move || store::preferences_tab().label().to_owned())
        .attributes(attrs)
        .open(store::preferences_open)
        .on_open_change(|change| {
            if !change.open {
                store::close_preferences();
            }
        })
        .content(preferences_surface)
}

fn preferences_surface() -> impl IntoElement {
    store::set_preferences_focused(true);
    current_window().on_focus_change(store::set_preferences_focused);

    view()
        .flex_1()
        .min_h(px(0.0))
        .bg(palette::canvas_bg())
        .text_color(palette::text_primary())
        .child(preferences_header())
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .child(when(
                    || store::preferences_tab() == PreferencesTab::General,
                    general_tab,
                ))
                .child(when(
                    || store::preferences_tab() == PreferencesTab::Appearance,
                    appearance_tab,
                ))
                .child(when(
                    || store::preferences_tab() == PreferencesTab::Providers,
                    providers_tab,
                ))
                .child(when(
                    || store::preferences_tab() == PreferencesTab::Machines,
                    machines_tab,
                ))
                .child(when(
                    || store::preferences_tab() == PreferencesTab::Extensions,
                    extensions_tab,
                ))
                .child(when(
                    || store::preferences_tab() == PreferencesTab::Advanced,
                    advanced_tab,
                )),
        )
}

// --- Header & Toolbar Tabs --------------------------------------------------------------

fn preferences_header() -> impl IntoElement {
    let mut tab_strip = view()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .pb(px(10.0))
        .window_drag(false);

    for tab in PreferencesTab::ALL {
        tab_strip = tab_strip.child(toolbar_tab_button(*tab));
    }

    view()
        .flex_shrink_0()
        .bg(palette::header_bg())
        .border_b_1()
        .border_color(palette::border_color())
        .window_drag(true)
        .child(
            view().h(px(36.0)).items_center().justify_center().child(
                text(move || store::preferences_tab().label().to_owned())
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_primary()),
            ),
        )
        .child(tab_strip)
}

fn toolbar_tab_button(tab: PreferencesTab) -> impl IntoElement {
    let is_active = move || store::preferences_tab() == tab;
    let tint = move || {
        if is_active() {
            palette::nav_active_bg()
        } else {
            palette::text_secondary()
        }
    };

    let test_id = match tab {
        PreferencesTab::General => "prefs-tab-general",
        PreferencesTab::Appearance => "prefs-tab-appearance",
        PreferencesTab::Providers => "prefs-tab-providers",
        PreferencesTab::Machines => "prefs-tab-machines",
        PreferencesTab::Extensions => "prefs-tab-extensions",
        PreferencesTab::Advanced => "prefs-tab-advanced",
    };

    button()
        .test_id(test_id)
        .accessibility_label(tab.label())
        .on_press(move || store::set_preferences_tab(tab))
        .min_w(px(74.0))
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(8.0))
        .items_center()
        .justify_center()
        .gap(px(4.0))
        .bg(move || {
            if is_active() {
                palette::prefs_tab_active_bg()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .hover(|s| s.bg(palette::sidebar_hover_bg()))
        .active(|s| s.opacity(0.8))
        .child(tab_icon(tab, tint))
        .child(
            text(tab.label())
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if is_active() {
                        palette::nav_active_bg()
                    } else {
                        palette::text_muted()
                    }
                }),
        )
}

fn tab_icon(tab: PreferencesTab, color: impl IntoReactive<AdaptiveColor> + 'static) -> AnyElement {
    match tab {
        PreferencesTab::General => icons::settings_gear(20.0, color).into_element(),
        PreferencesTab::Appearance => icons::appearance(color).into_element(),
        PreferencesTab::Providers => icons::providers(color).into_element(),
        PreferencesTab::Machines => icons::machines_toolbar(color).into_element(),
        PreferencesTab::Extensions => icons::extensions(color).into_element(),
        PreferencesTab::Advanced => icons::advanced(color).into_element(),
    }
}

// --- Providers Tab (matching 17 Preferences.png) ----------------------------------------

fn providers_tab() -> impl IntoElement {
    let active_editor: Signal<Option<ProviderEditor>> = signal(None);
    let add_menu_open = signal(false);

    // Form draft signals for inline editors.
    let draft_primary = signal(String::new());
    let draft_secondary = signal(String::new());
    let draft_cap = signal(String::new());
    let draft_model = signal(String::new());
    let claude_spawn_ui = ClaudeSpawnUiState::new();
    let codex_spawn_ui = CodexSpawnUiState::new();

    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(22.0))
            .gap(px(18.0))
            .child(
                view()
                    .flex_row()
                    .items_start()
                    .gap(px(18.0))
                    .child(providers_left_rail(
                        active_editor,
                        add_menu_open,
                        draft_primary,
                        draft_secondary,
                        draft_cap,
                        draft_model,
                    ))
                    .child(providers_detail_column(
                        active_editor,
                        add_menu_open,
                        draft_primary,
                        draft_secondary,
                        draft_cap,
                        draft_model,
                        claude_spawn_ui,
                        codex_spawn_ui,
                    )),
            )
            .child(
                text(
                    "Claude and Codex use your existing system CLI logins on PATH (or a custom binary path if installed elsewhere). OpenRouter keys stay in the macOS Keychain. Workflow and agent files name a provider, never a key, so they stay portable; each project picks its OpenRouter key in Project Settings › Providers.",
                )
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
            ),
    )
}

fn providers_left_rail(
    active_editor: Signal<Option<ProviderEditor>>,
    add_menu_open: Signal<bool>,
    draft_primary: Signal<String>,
    draft_secondary: Signal<String>,
    draft_cap: Signal<String>,
    draft_model: Signal<String>,
) -> impl IntoElement {
    view()
        .w(px(236.0))
        .flex_shrink_0()
        .gap(px(10.0))
        .child(dynamic(move || {
            let prefs = store::preferences();
            let selected_id = store::selected_provider_id();
            let is_adding_account = matches!(
                active_editor.get(),
                Some(ProviderEditor::AddOpenRouterKey | ProviderEditor::AddCodexAccount)
            );

            let mut card = view()
                .rounded(px(10.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_bg())
                .p(px(10.0))
                .gap(px(10.0));

            // Claude group
            card = card
                .child(provider_group_header(ProviderKind::Claude))
                .child(provider_list_row(
                    prefs.claude.id.clone(),
                    ProviderKind::Claude.badge(),
                    prefs.claude.display_name.clone(),
                    prefs.claude.list_subtitle(),
                    false,
                    selected_id == prefs.claude.id && !is_adding_account,
                    active_editor,
                    add_menu_open,
                ));

            // Codex group
            card = card.child(provider_group_header(ProviderKind::Codex));
            if let Some(codex) = &prefs.codex {
                card = card.child(provider_list_row(
                    codex.id.clone(),
                    ProviderKind::Codex.badge(),
                    codex.display_name.clone(),
                    codex.list_subtitle(),
                    false,
                    selected_id == codex.id && !is_adding_account,
                    active_editor,
                    add_menu_open,
                ));
            } else {
                card = card.child(
                    text("No Codex account configured")
                        .px(px(6.0))
                        .py(px(4.0))
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                );
            }

            // OpenRouter group
            card = card.child(provider_group_header(ProviderKind::OpenRouter));
            if prefs.openrouter_keys.is_empty() {
                card = card.child(
                    text("No OpenRouter keys configured")
                        .px(px(6.0))
                        .py(px(4.0))
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                );
            } else {
                let mut or_list = view().gap(px(4.0));
                for key in &prefs.openrouter_keys {
                    or_list = or_list.child(provider_list_row(
                        key.id.clone(),
                        ProviderKind::OpenRouter.badge(),
                        key.name.clone(),
                        key.list_subtitle(),
                        key.is_default,
                        selected_id == key.id && !is_adding_account,
                        active_editor,
                        add_menu_open,
                    ));
                }
                card = card.child(or_list);
            }

            card
        }))
        .child(
            view()
                .items_start()
                .gap(px(6.0))
                .child(
                    button()
                        .accessibility_label("Add Account")
                        .on_press(move || add_menu_open.update(|open| *open = !*open))
                        .px(px(11.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(5.0))
                                .child(
                                    text("Add Account")
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(palette::text_primary()),
                                )
                                .child(icons::caret_down(palette::text_secondary())),
                        ),
                )
                .child(when(
                    move || add_menu_open.get(),
                    move || {
                        dynamic(move || {
                            let has_codex = store::preferences().codex.is_some();
                            let codex_option = if has_codex {
                                menu_option_button("Manage Codex CLI account", move || {
                                    add_menu_open.set(false);
                                    active_editor.set(None);
                                    store::select_provider("codex".to_owned());
                                })
                                .into_element()
                            } else {
                                menu_option_button("Add Codex CLI account...", move || {
                                    add_menu_open.set(false);
                                    store::clear_preferences_messages();
                                    draft_primary.set("codex".to_owned());
                                    draft_model.set("gpt-5.3-codex".to_owned());
                                    active_editor.set(Some(ProviderEditor::AddCodexAccount));
                                })
                                .into_element()
                            };

                            view()
                                .w_full()
                                .rounded(px(9.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .p(px(4.0))
                                .gap(px(2.0))
                                .child(menu_option_button("Add OpenRouter key...", move || {
                                    add_menu_open.set(false);
                                    store::clear_preferences_messages();
                                    draft_primary.set(String::new());
                                    draft_secondary.set(String::new());
                                    draft_cap.set("50".to_owned());
                                    draft_model.set("openrouter/auto".to_owned());
                                    active_editor.set(Some(ProviderEditor::AddOpenRouterKey));
                                }))
                                .child(menu_option_button("Manage Claude CLI account", move || {
                                    add_menu_open.set(false);
                                    active_editor.set(None);
                                    store::select_provider("claude".to_owned());
                                }))
                                .child(codex_option)
                        })
                    },
                )),
        )
}

fn provider_group_header(kind: ProviderKind) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .px(px(4.0))
        .child(
            text(kind.label())
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            text(format!("· {}", kind.group_hint()))
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
}

#[allow(clippy::too_many_arguments)]
fn provider_list_row(
    id: String,
    badge: &'static str,
    title: String,
    subtitle: String,
    is_default: bool,
    selected: bool,
    active_editor: Signal<Option<ProviderEditor>>,
    add_menu_open: Signal<bool>,
) -> impl IntoElement {
    let row_bg = if selected {
        palette::nav_active_bg()
    } else {
        AdaptiveColor::TRANSPARENT
    };
    let badge_bg = if selected {
        palette::provider_badge_selected_bg()
    } else {
        palette::provider_badge_bg()
    };
    let title_color = if selected {
        palette::nav_active_fg()
    } else {
        palette::text_primary()
    };
    let subtitle_color = if selected {
        AdaptiveColor::new(rgba(0xFFFFFFD9), rgba(0xFFFFFFD9))
    } else {
        palette::text_muted()
    };

    button()
        .accessibility_label(title.clone())
        .on_press(move || {
            add_menu_open.set(false);
            active_editor.set(None);
            store::select_provider(id.clone());
        })
        .w_full()
        .rounded(px(8.0))
        .px(px(8.0))
        .py(px(7.0))
        .bg(row_bg)
        .hover(move |s| {
            if selected {
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
                .gap(px(9.0))
                .child(
                    view()
                        .w(px(28.0))
                        .h(px(28.0))
                        .rounded(px(7.0))
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .bg(badge_bg)
                        .child(
                            text(badge)
                                .text_size(px(11.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(title_color),
                        ),
                )
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(1.5))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    text(title)
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(title_color),
                                )
                                .child(when(
                                    move || is_default,
                                    move || {
                                        view()
                                            .px(px(5.5))
                                            .py(px(1.0))
                                            .rounded(px(4.5))
                                            .border_1()
                                            .border_color(if selected {
                                                AdaptiveColor::new(
                                                    rgba(0xFFFFFF66),
                                                    rgba(0xFFFFFF66),
                                                )
                                            } else {
                                                palette::border_color()
                                            })
                                            .bg(if selected {
                                                AdaptiveColor::new(
                                                    rgba(0xFFFFFF22),
                                                    rgba(0xFFFFFF22),
                                                )
                                            } else {
                                                palette::card_inset_bg()
                                            })
                                            .child(
                                                text("Default")
                                                    .text_size(px(10.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(if selected {
                                                        palette::nav_active_fg()
                                                    } else {
                                                        palette::text_secondary()
                                                    }),
                                            )
                                    },
                                )),
                        )
                        .child(
                            text(subtitle)
                                .text_size(px(11.5))
                                .text_color(subtitle_color),
                        ),
                ),
        )
}

fn menu_option_button(label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .on_press(on_press)
        .w_full()
        .px(px(9.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .items_start()
        .hover(|s| s.bg(palette::sidebar_hover_bg()))
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
}

#[allow(clippy::too_many_arguments)]
fn providers_detail_column(
    active_editor: Signal<Option<ProviderEditor>>,
    add_menu_open: Signal<bool>,
    draft_primary: Signal<String>,
    draft_secondary: Signal<String>,
    draft_cap: Signal<String>,
    draft_model: Signal<String>,
    claude_spawn_ui: ClaudeSpawnUiState,
    codex_spawn_ui: CodexSpawnUiState,
) -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(14.0))
        .child(dynamic(move || {
            let editor = active_editor.get();
            if editor == Some(ProviderEditor::AddOpenRouterKey) {
                return add_openrouter_key_card(
                    active_editor,
                    draft_primary,
                    draft_secondary,
                    draft_cap,
                    draft_model,
                )
                .into_element();
            }
            if editor == Some(ProviderEditor::AddCodexAccount) {
                return add_codex_account_card(active_editor, draft_primary, draft_model)
                    .into_element();
            }

            let prefs = store::preferences();
            let selected_id = store::selected_provider_id();
            if let Some(key) = prefs.openrouter_key(&selected_id).cloned() {
                openrouter_detail_pane(
                    key,
                    active_editor,
                    add_menu_open,
                    draft_primary,
                    draft_cap,
                    draft_model,
                )
                .into_element()
            } else if let Some(account) = prefs.cli_account(&selected_id).cloned() {
                cli_account_detail_pane(
                    account,
                    active_editor,
                    add_menu_open,
                    draft_primary,
                    draft_model,
                    claude_spawn_ui,
                    codex_spawn_ui,
                )
                .into_element()
            } else {
                cli_account_detail_pane(
                    prefs.claude,
                    active_editor,
                    add_menu_open,
                    draft_primary,
                    draft_model,
                    claude_spawn_ui,
                    codex_spawn_ui,
                )
                .into_element()
            }
        }))
}

fn add_openrouter_key_card(
    active_editor: Signal<Option<ProviderEditor>>,
    draft_name: Signal<String>,
    draft_key: Signal<String>,
    draft_cap: Signal<String>,
    draft_model: Signal<String>,
) -> impl IntoElement {
    let submit = move || {
        let name = draft_name.get_untracked();
        let api_key = draft_key.get_untracked();
        let cap = draft_cap.get_untracked();
        let model = draft_model.get_untracked();
        if store::add_openrouter_key(&name, &api_key, &cap, &model) {
            active_editor.set(None);
        }
    };

    view()
        .gap(px(12.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(detail_badge("OR"))
                .child(
                    view()
                        .gap(px(1.5))
                        .child(
                            text("Add OpenRouter API key")
                                .text_size(px(15.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text("Stored in the macOS Keychain · selectable per project")
                                .text_size(px(12.0))
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(
            grouped_card("New key").child(card_divider()).child(
                view()
                    .p(px(16.0))
                    .gap(px(12.0))
                    .child(form_field(
                        "Key name",
                        input()
                            .test_id("prefs-new-key-name")
                            .value(move || draft_name.get())
                            .on_change(move |v: &str| draft_name.set(v.to_owned()))
                            .placeholder("e.g. Client team or CI")
                            .px(px(10.0))
                            .py(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_inset_bg())
                            .rounded(px(7.0))
                            .text_color(palette::text_primary()),
                    ))
                    .child(form_field(
                        "API key",
                        input()
                            .test_id("prefs-new-key-secret")
                            .value(move || draft_key.get())
                            .on_change(move |v: &str| draft_key.set(v.to_owned()))
                            .placeholder("sk-or-v1-...")
                            .app_class("font-mono")
                            .px(px(10.0))
                            .py(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_inset_bg())
                            .rounded(px(7.0))
                            .text_color(palette::text_primary()),
                    ))
                    .child(
                        view()
                            .flex_row()
                            .gap(px(12.0))
                            .child(
                                view().flex_1().child(form_field(
                                    "Monthly spending cap (USD)",
                                    input()
                                        .test_id("prefs-new-key-cap")
                                        .value(move || draft_cap.get())
                                        .on_change(move |v: &str| draft_cap.set(v.to_owned()))
                                        .placeholder("Blank for no cap")
                                        .px(px(10.0))
                                        .py(px(7.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::card_inset_bg())
                                        .rounded(px(7.0))
                                        .text_color(palette::text_primary()),
                                )),
                            )
                            .child(
                                view().flex_1().child(form_field(
                                    "Default model",
                                    input()
                                        .test_id("prefs-new-key-model")
                                        .value(move || draft_model.get())
                                        .on_change(move |v: &str| draft_model.set(v.to_owned()))
                                        .placeholder("openrouter/auto")
                                        .app_class("font-mono")
                                        .px(px(10.0))
                                        .py(px(7.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::card_inset_bg())
                                        .rounded(px(7.0))
                                        .text_color(palette::text_primary()),
                                )),
                            ),
                    )
                    .child(preferences_error_banner())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(secondary_button("Cancel", move || {
                                store::clear_preferences_messages();
                                active_editor.set(None);
                            }))
                            .child(primary_button("Add Key", submit)),
                    ),
            ),
        )
}

fn add_codex_account_card(
    active_editor: Signal<Option<ProviderEditor>>,
    draft_binary: Signal<String>,
    draft_model: Signal<String>,
) -> impl IntoElement {
    let submit = move || {
        let binary = draft_binary.get_untracked();
        let model = draft_model.get_untracked();
        if store::add_codex_account(&binary, &model) {
            active_editor.set(None);
        }
    };

    view()
        .gap(px(12.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(detail_badge(ProviderKind::Codex.badge()))
                .child(
                    view()
                        .gap(px(1.5))
                        .child(
                            text("Add Codex CLI account")
                                .text_size(px(15.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(
                                "Uses your existing system Codex CLI login on PATH (or a custom binary path)",
                            )
                            .text_size(px(12.0))
                            .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(
            grouped_card("Codex CLI").child(card_divider()).child(
                view()
                    .p(px(16.0))
                    .gap(px(12.0))
                    .child(form_field(
                        "CLI binary",
                        input()
                            .test_id("prefs-new-codex-binary")
                            .value(move || draft_binary.get())
                            .on_change(move |v: &str| draft_binary.set(v.to_owned()))
                            .placeholder("codex")
                            .app_class("font-mono")
                            .px(px(10.0))
                            .py(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_inset_bg())
                            .rounded(px(7.0))
                            .text_color(palette::text_primary()),
                    ))
                    .child(form_field(
                        "Default model",
                        input()
                            .test_id("prefs-new-codex-model")
                            .value(move || draft_model.get())
                            .on_change(move |v: &str| draft_model.set(v.to_owned()))
                            .placeholder("gpt-5.3-codex")
                            .app_class("font-mono")
                            .px(px(10.0))
                            .py(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_inset_bg())
                            .rounded(px(7.0))
                            .text_color(palette::text_primary()),
                    ))
                    .child(preferences_error_banner())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(secondary_button("Cancel", move || {
                                store::clear_preferences_messages();
                                active_editor.set(None);
                            }))
                            .child(
                                button()
                                    .test_id("prefs-new-codex-submit")
                                    .on_press(submit)
                                    .px(px(13.0))
                                    .py(px(6.0))
                                    .rounded(px(7.0))
                                    .bg(palette::nav_active_bg())
                                    .active(|s| s.opacity(0.8))
                                    .child(
                                        text("Add Codex")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::nav_active_fg()),
                                    ),
                            ),
                    ),
            ),
        )
}

fn openrouter_detail_pane(
    key: OpenRouterKey,
    active_editor: Signal<Option<ProviderEditor>>,
    add_menu_open: Signal<bool>,
    draft_primary: Signal<String>,
    draft_cap: Signal<String>,
    draft_model: Signal<String>,
) -> impl IntoElement {
    let key_id_replace = key.id.clone();
    let key_id_cap = key.id.clone();
    let key_id_model = key.id.clone();
    let key_id_rename = key.id.clone();
    let key_id_remove = key.id.clone();

    let current_cap_text = key
        .monthly_cap_cents
        .map(format_whole_or_cents)
        .map(|s| s.trim_start_matches('$').to_owned())
        .unwrap_or_default();
    let current_model = key.default_model.clone();
    let current_name = key.name.clone();
    let current_name_for_confirm = key.name.clone();
    let is_default = key.is_default;
    let editor = active_editor.get();

    let is_replacing_key = editor == Some(ProviderEditor::ReplaceKey(key.id.clone()));
    let is_editing_cap = editor == Some(ProviderEditor::EditCap(key.id.clone()));
    let is_changing_model = editor == Some(ProviderEditor::ChangeModel(key.id.clone()));
    let is_renaming = editor == Some(ProviderEditor::RenameKey(key.id.clone()));
    let is_confirming_remove = editor == Some(ProviderEditor::ConfirmRemove(key.id.clone()));

    view()
        .gap(px(14.0))
        .child(detail_header(
            ProviderKind::OpenRouter.badge(),
            key.name.clone(),
            "OpenRouter · API key".to_owned(),
            &key.test_state,
            "Key works",
        ))
        .child(
            grouped_card("Key")
                .child(card_divider())
                .child(
                    view()
                        .gap(px(8.0))
                        .px(px(16.0))
                        .py(px(11.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child(
                                    view()
                                        .flex_1()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(12.0))
                                        .child(row_label("API key"))
                                        .child(
                                            text(key.masked_key())
                                                .app_class("font-mono")
                                                .text_size(px(12.5))
                                                .text_color(palette::text_primary()),
                                        ),
                                )
                                .child(secondary_button("Replace...", move || {
                                    add_menu_open.set(false);
                                    store::clear_preferences_messages();
                                    draft_primary.set(String::new());
                                    active_editor.set(Some(ProviderEditor::ReplaceKey(
                                        key_id_replace.clone(),
                                    )));
                                })),
                        )
                        .child(when(
                            move || is_replacing_key,
                            move || {
                                inline_single_field_editor(
                                    draft_primary,
                                    "Enter new OpenRouter API key",
                                    true,
                                    "Save Key",
                                    move || {
                                        if store::replace_selected_openrouter_key(
                                            &draft_primary.get_untracked(),
                                        ) {
                                            active_editor.set(None);
                                        }
                                    },
                                    move || {
                                        store::clear_preferences_messages();
                                        active_editor.set(None);
                                    },
                                )
                            },
                        )),
                )
                .child(card_divider())
                .child(
                    view()
                        .gap(px(8.0))
                        .px(px(16.0))
                        .py(px(11.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child(
                                    view()
                                        .flex_1()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(12.0))
                                        .child(row_label("Monthly spending cap"))
                                        .child(
                                            text(key.cap_summary())
                                                .text_size(px(13.0))
                                                .tabular_nums()
                                                .text_color(palette::text_primary()),
                                        ),
                                )
                                .child(secondary_button("Edit...", move || {
                                    add_menu_open.set(false);
                                    store::clear_preferences_messages();
                                    draft_cap.set(current_cap_text.clone());
                                    active_editor
                                        .set(Some(ProviderEditor::EditCap(key_id_cap.clone())));
                                })),
                        )
                        .child(when(
                            move || is_editing_cap,
                            move || {
                                inline_single_field_editor(
                                    draft_cap,
                                    "Monthly cap in USD (blank for no cap)",
                                    false,
                                    "Save Cap",
                                    move || {
                                        if store::update_selected_openrouter_cap(
                                            &draft_cap.get_untracked(),
                                        ) {
                                            active_editor.set(None);
                                        }
                                    },
                                    move || {
                                        store::clear_preferences_messages();
                                        active_editor.set(None);
                                    },
                                )
                            },
                        )),
                )
                .child(card_divider())
                .child(
                    view().px(px(16.0)).py(px(11.0)).child(
                        text(key.cap_note.clone())
                            .text_size(px(12.0))
                            .text_color(palette::text_muted()),
                    ),
                ),
        )
        .child(
            grouped_card("Defaults for this key")
                .child(card_divider())
                .child(
                    view()
                        .gap(px(8.0))
                        .px(px(16.0))
                        .py(px(11.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child(
                                    view()
                                        .flex_1()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(12.0))
                                        .child(row_label("Model"))
                                        .child(
                                            text(key.default_model.clone())
                                                .app_class("font-mono")
                                                .text_size(px(12.5))
                                                .text_color(palette::text_primary()),
                                        ),
                                )
                                .child(secondary_button("Change...", move || {
                                    add_menu_open.set(false);
                                    store::clear_preferences_messages();
                                    draft_model.set(current_model.clone());
                                    active_editor.set(Some(ProviderEditor::ChangeModel(
                                        key_id_model.clone(),
                                    )));
                                })),
                        )
                        .child(when(
                            move || is_changing_model,
                            move || {
                                model_picker_editor(
                                    draft_model,
                                    &[
                                        "openrouter/auto",
                                        "anthropic/claude-sonnet-4",
                                        "openai/gpt-5",
                                        "google/gemini-2.5-pro",
                                    ],
                                    active_editor,
                                )
                            },
                        )),
                ),
        )
        .child(
            grouped_card("Used by")
                .child(card_divider())
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .px(px(16.0))
                        .py(px(11.0))
                        .child(row_label("Default OpenRouter key"))
                        .child(
                            text(if is_default { "Yes" } else { "No" })
                                .text_size(px(13.0))
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
                        .child(row_label("Projects"))
                        .child(
                            text(key.used_by_projects_display())
                                .text_size(px(13.0))
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
        .child(preferences_error_banner())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    button()
                        .disabled(is_default)
                        .on_press(store::make_selected_openrouter_default)
                        .px(px(12.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .disabled_style(|s| s.opacity(0.45))
                        .child(
                            text("Make Default")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(secondary_button("Rename...", move || {
                    add_menu_open.set(false);
                    store::clear_preferences_messages();
                    draft_primary.set(current_name.clone());
                    active_editor.set(Some(ProviderEditor::RenameKey(key_id_rename.clone())));
                }))
                .child(
                    button()
                        .on_press(move || {
                            add_menu_open.set(false);
                            store::clear_preferences_messages();
                            active_editor
                                .set(Some(ProviderEditor::ConfirmRemove(key_id_remove.clone())));
                        })
                        .px(px(12.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Remove...")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::destructive_text()),
                        ),
                ),
        )
        .child(when(
            move || is_renaming,
            move || {
                inline_single_field_editor(
                    draft_primary,
                    "New key name",
                    false,
                    "Save Name",
                    move || {
                        if store::rename_selected_openrouter_key(&draft_primary.get_untracked()) {
                            active_editor.set(None);
                        }
                    },
                    move || {
                        store::clear_preferences_messages();
                        active_editor.set(None);
                    },
                )
            },
        ))
        .child(when(
            move || is_confirming_remove,
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .p(px(12.0))
                    .rounded(px(9.0))
                    .border_1()
                    .border_color(palette::destructive_text())
                    .bg(palette::card_bg())
                    .child(
                        text(format!(
                            "Remove '{}' from Keychain and Bruk?",
                            current_name_for_confirm
                        ))
                        .text_size(px(12.5))
                        .text_color(palette::text_primary()),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(secondary_button("Cancel", move || {
                                active_editor.set(None);
                            }))
                            .child(
                                button()
                                    .on_press(move || {
                                        store::remove_selected_openrouter_key();
                                        active_editor.set(None);
                                    })
                                    .px(px(12.0))
                                    .py(px(6.0))
                                    .rounded(px(7.0))
                                    .bg(palette::destructive_text())
                                    .active(|s| s.opacity(0.8))
                                    .child(
                                        text("Remove Key")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::nav_active_fg()),
                                    ),
                            ),
                    )
            },
        ))
        .child(preferences_feedback_caption())
}

fn cli_account_detail_pane(
    account: CliAccount,
    active_editor: Signal<Option<ProviderEditor>>,
    add_menu_open: Signal<bool>,
    draft_cli_path: Signal<String>,
    draft_model: Signal<String>,
    claude_spawn_ui: ClaudeSpawnUiState,
    codex_spawn_ui: CodexSpawnUiState,
) -> impl IntoElement {
    let provider = account.provider;
    let is_claude = provider == ProviderKind::Claude;
    let is_codex = provider == ProviderKind::Codex;
    let provider_slug = match provider {
        ProviderKind::Claude => "claude",
        ProviderKind::Codex => "codex",
        ProviderKind::OpenRouter => "openrouter",
    };
    let account_id_model = account.id.clone();
    let account_id_remove = account.id.clone();
    let account_id_path = account.id.clone();
    let account_id_path_cta = account.id.clone();
    let current_model = account.default_model.clone();
    let current_custom_path = account.custom_cli_path.clone().unwrap_or_default();
    let current_custom_path_cta = current_custom_path.clone();
    let has_custom_path = account.custom_cli_path.is_some();
    let connected = account.connected;
    let binary_found = account.binary_found;
    let uses_system_login = account.uses_system_login;
    let is_changing_model =
        active_editor.get() == Some(ProviderEditor::ChangeModel(account.id.clone()));
    let is_editing_cli_path =
        active_editor.get() == Some(ProviderEditor::EditCliPath(account.id.clone()));
    let is_confirming_remove =
        active_editor.get() == Some(ProviderEditor::ConfirmRemove(account.id.clone()));

    let total_available_models = account.available_models.len();
    let visible_models = account.visible_available_models();
    let model_families: Vec<(String, bool)> = account
        .available_model_families()
        .into_iter()
        .map(|family| {
            let visible = account.is_model_family_visible(&family);
            (family, visible)
        })
        .collect();
    let has_model_families = !model_families.is_empty();
    let hidden_family_names: Vec<String> = model_families
        .iter()
        .filter(|(_, visible)| !*visible)
        .map(|(family, _)| family.clone())
        .collect();
    let usage_summary = account.usage.clone();
    let usage_warning = account.usage_warning.clone();

    let model_presets: Vec<&'static str> = match provider {
        ProviderKind::Claude => ["claude-sonnet-4-6", "claude-opus-4-6", "claude-haiku-4-5"]
            .into_iter()
            .filter(|id| {
                model_family_from_id(id)
                    .map(|family| account.is_model_family_visible(&family))
                    .unwrap_or(true)
            })
            .collect(),
        ProviderKind::Codex => ["gpt-5.3-codex", "gpt-5.2-codex", "o4-mini"]
            .into_iter()
            .filter(|id| {
                model_family_from_id(id)
                    .map(|family| account.is_model_family_visible(&family))
                    .unwrap_or(true)
            })
            .collect(),
        ProviderKind::OpenRouter => vec!["openrouter/auto"],
    };

    let projects_display = if account.used_by_projects.is_empty() {
        "None".to_owned()
    } else {
        account.used_by_projects.join(" · ")
    };

    let account_display = if connected {
        if account.email.is_empty() {
            "Signed in".to_owned()
        } else {
            account.email.clone()
        }
    } else if !binary_found {
        "CLI not found".to_owned()
    } else {
        "Not signed in".to_owned()
    };

    let subscription_display = if connected && !account.plan.trim().is_empty() {
        account.plan.clone()
    } else {
        "—".to_owned()
    };

    let cli_binary_row: AnyElement = {
        let binary_text = if binary_found {
            account.cli_binary.clone()
        } else if let Some(custom) = &account.custom_cli_path {
            format!("{custom} (not found)")
        } else {
            "Not found on default PATH".to_owned()
        };
        let path_badge_label = if has_custom_path {
            "Custom"
        } else if binary_found {
            "PATH"
        } else {
            ""
        };
        let edit_btn_label = if binary_found {
            "Change..."
        } else {
            "Set Path..."
        };
        let path_edit_test_id = format!("prefs-{provider_slug}-path-edit");

        view()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(11.0))
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
                            .flex_row()
                            .items_center()
                            .gap(px(12.0))
                            .child(row_label("CLI binary"))
                            .child(
                                view()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .flex_row()
                                    .items_center()
                                    .flex_wrap()
                                    .gap(px(6.0))
                                    .child(
                                        text(binary_text)
                                            .app_class("font-mono")
                                            .text_size(px(12.5))
                                            .text_color(if binary_found {
                                                palette::text_primary()
                                            } else {
                                                palette::destructive_text()
                                            }),
                                    )
                                    .child(when(
                                        move || !path_badge_label.is_empty(),
                                        move || {
                                            view()
                                                .px(px(6.0))
                                                .py(px(1.5))
                                                .rounded(px(4.5))
                                                .border_1()
                                                .border_color(palette::border_color())
                                                .bg(palette::card_inset_bg())
                                                .child(
                                                    text(path_badge_label)
                                                        .text_size(px(10.5))
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .text_color(palette::text_secondary()),
                                                )
                                        },
                                    )),
                            ),
                    )
                    .child(
                        button()
                            .test_id(&path_edit_test_id)
                            .on_press(move || {
                                add_menu_open.set(false);
                                store::clear_preferences_messages();
                                draft_cli_path.set(current_custom_path.clone());
                                active_editor.set(Some(ProviderEditor::EditCliPath(
                                    account_id_path.clone(),
                                )));
                            })
                            .px(px(12.0))
                            .py(px(5.5))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::control_bg())
                            .hover(|s| s.bg(palette::subtle_surface_bg()))
                            .active(|s| s.opacity(0.8))
                            .child(
                                text(edit_btn_label)
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_primary()),
                            ),
                    ),
            )
            .child(when(
                move || is_editing_cli_path,
                move || cli_path_editor(provider, draft_cli_path, has_custom_path, active_editor),
            ))
            .into_element()
    };

    let remove_codex_button = move || {
        let remove_id = account_id_remove.clone();
        button()
            .test_id("prefs-codex-remove")
            .on_press(move || {
                add_menu_open.set(false);
                store::clear_preferences_messages();
                active_editor.set(Some(ProviderEditor::ConfirmRemove(remove_id.clone())));
            })
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::control_bg())
            .hover(|s| s.bg(palette::subtle_surface_bg()))
            .active(|s| s.opacity(0.8))
            .child(
                text("Remove...")
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::destructive_text()),
            )
    };

    let bottom_actions: AnyElement = {
        let mut row = view().flex_row().items_center().gap(px(8.0));
        if connected {
            let status_copy = if uses_system_login {
                format!("Using your existing system {} CLI login.", provider.label())
            } else {
                format!("Signed in to {} CLI.", provider.label())
            };
            row = row.child(
                text(status_copy)
                    .text_size(px(12.0))
                    .text_color(palette::text_muted()),
            );
        } else if binary_found {
            let sign_in_test_id = format!("prefs-{provider_slug}-sign-in");
            let sign_in_busy = move || {
                if is_claude {
                    store::claude_sign_in_in_progress()
                } else {
                    store::codex_sign_in_in_progress()
                }
            };
            row = row.child(
                button()
                    .test_id(&sign_in_test_id)
                    .disabled(sign_in_busy)
                    .on_press(move || {
                        if is_claude {
                            store::sign_in_claude();
                        } else {
                            store::sign_in_codex();
                        }
                    })
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(px(7.0))
                    .bg(palette::nav_active_bg())
                    .active(|s| s.opacity(0.8))
                    .disabled_style(|s| s.opacity(0.5))
                    .child(
                        text(move || {
                            if sign_in_busy() {
                                "Signing in...".to_owned()
                            } else {
                                "Sign In".to_owned()
                            }
                        })
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::nav_active_fg()),
                    ),
            );
        } else {
            let set_path_test_id = format!("prefs-{provider_slug}-set-path-cta");
            let set_path_label = format!("Set {} Path...", provider.label());
            row = row.child(
                button()
                    .test_id(&set_path_test_id)
                    .on_press(move || {
                        add_menu_open.set(false);
                        store::clear_preferences_messages();
                        draft_cli_path.set(current_custom_path_cta.clone());
                        active_editor.set(Some(ProviderEditor::EditCliPath(
                            account_id_path_cta.clone(),
                        )));
                    })
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(px(7.0))
                    .bg(palette::nav_active_bg())
                    .active(|s| s.opacity(0.8))
                    .child(
                        text(set_path_label)
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::nav_active_fg()),
                    ),
            );
        }

        if is_codex {
            row = row.child(remove_codex_button());
        }

        row.into_element()
    };

    view()
        .gap(px(14.0))
        .child(detail_header(
            account.provider.badge(),
            account.display_name.clone(),
            account.header_subtitle(),
            &account.test_state,
            "CLI works",
        ))
        .child(
            grouped_card("CLI Account")
                .child(card_divider())
                .child(static_row("Account", account_display, false))
                .child(card_divider())
                .child(static_row("Subscription", subscription_display, false))
                .child(card_divider())
                .child(cli_binary_row)
                .child(card_divider())
                .child(static_row(
                    "Config folder",
                    account.config_dir.clone(),
                    true,
                )),
        )
        .child(when(
            move || connected,
            move || {
                let loading = if is_claude {
                    store::claude_details_loading()
                } else {
                    store::codex_details_loading()
                };
                cli_usage_card(
                    provider,
                    usage_summary.clone(),
                    usage_warning.clone(),
                    loading,
                )
            },
        ))
        .child(
            grouped_card("Defaults for this provider")
                .child(card_divider())
                .child(
                    view()
                        .gap(px(8.0))
                        .px(px(16.0))
                        .py(px(11.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child(
                                    view()
                                        .flex_1()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(12.0))
                                        .child(row_label("Model"))
                                        .child(
                                            text(account.default_model.clone())
                                                .app_class("font-mono")
                                                .text_size(px(12.5))
                                                .text_color(palette::text_primary()),
                                        ),
                                )
                                .child(
                                    button()
                                        .test_id("prefs-cli-model-change")
                                        .on_press(move || {
                                            add_menu_open.set(false);
                                            store::clear_preferences_messages();
                                            draft_model.set(current_model.clone());
                                            active_editor.set(Some(ProviderEditor::ChangeModel(
                                                account_id_model.clone(),
                                            )));
                                        })
                                        .px(px(12.0))
                                        .py(px(5.5))
                                        .rounded(px(7.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::control_bg())
                                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                                        .active(|s| s.opacity(0.8))
                                        .child(
                                            text("Change...")
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(palette::text_primary()),
                                        ),
                                ),
                        )
                        .child(when(
                            move || is_changing_model,
                            move || {
                                if total_available_models > 0 {
                                    cli_dynamic_model_picker(
                                        provider,
                                        draft_model,
                                        visible_models.clone(),
                                        total_available_models,
                                        hidden_family_names.clone(),
                                        active_editor,
                                    )
                                    .into_element()
                                } else {
                                    model_picker_editor(draft_model, &model_presets, active_editor)
                                        .into_element()
                                }
                            },
                        )),
                )
                .child(when(
                    move || has_model_families,
                    move || {
                        view()
                            .child(card_divider())
                            .child(cli_model_family_filter_row(
                                provider,
                                model_families.clone(),
                            ))
                    },
                )),
        )
        .child(when(
            move || is_claude,
            move || claude_spawn_settings_card(claude_spawn_ui),
        ))
        .child(when(
            move || is_codex,
            move || codex_spawn_settings_card(codex_spawn_ui),
        ))
        .child(
            grouped_card("Used by")
                .child(card_divider())
                .child(static_row("Projects", projects_display, false)),
        )
        .child(preferences_error_banner())
        .child(bottom_actions)
        .child(when(
            move || is_codex && is_confirming_remove,
            move || {
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .p(px(12.0))
                    .rounded(px(9.0))
                    .border_1()
                    .border_color(palette::destructive_text())
                    .bg(palette::card_bg())
                    .child(
                        text("Remove Codex CLI account from Bruk?")
                            .text_size(px(12.5))
                            .text_color(palette::text_primary()),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(secondary_button("Cancel", move || {
                                active_editor.set(None);
                            }))
                            .child(
                                button()
                                    .test_id("prefs-codex-confirm-remove")
                                    .on_press(move || {
                                        store::remove_codex_account();
                                        active_editor.set(None);
                                    })
                                    .px(px(12.0))
                                    .py(px(6.0))
                                    .rounded(px(7.0))
                                    .bg(palette::destructive_text())
                                    .active(|s| s.opacity(0.8))
                                    .child(
                                        text("Remove Account")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::nav_active_fg()),
                                    ),
                            ),
                    )
            },
        ))
        .child(preferences_feedback_caption())
}

fn cli_usage_card(
    provider: ProviderKind,
    usage: Option<ClaudeUsageSummary>,
    warning: Option<String>,
    loading: bool,
) -> impl IntoElement {
    let is_claude = provider == ProviderKind::Claude;
    let refresh_test_id = if is_claude {
        "prefs-claude-usage-refresh"
    } else {
        "prefs-codex-usage-refresh"
    };
    let mut card = view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(16.0))
                .py(px(9.0))
                .child(
                    text("Usage remaining")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .test_id(refresh_test_id)
                        .disabled(loading)
                        .on_press(move || {
                            if is_claude {
                                store::refresh_claude_details_async();
                            } else {
                                store::refresh_codex_details_async();
                            }
                        })
                        .px(px(9.0))
                        .py(px(3.5))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .disabled_style(|s| s.opacity(0.5))
                        .child(
                            text(if loading { "Refreshing..." } else { "Refresh" })
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        );

    match usage {
        Some(summary) if summary.rate_limits_available && !summary.windows.is_empty() => {
            for window in &summary.windows {
                card = card
                    .child(card_divider())
                    .child(cli_usage_window_row(window));
            }
            let resets = summary.upcoming_resets();
            if !resets.is_empty() {
                card = card
                    .child(card_divider())
                    .child(cli_quota_resets_section(&resets));
            }
            if let Some(extra) = summary.extra_usage_note {
                card = card.child(card_divider()).child(
                    view().px(px(16.0)).py(px(10.0)).child(
                        text(extra)
                            .text_size(px(12.0))
                            .text_color(palette::text_muted()),
                    ),
                );
            }
        }
        Some(_) => {
            card = card.child(card_divider()).child(
                view().px(px(16.0)).py(px(11.0)).child(
                    text("Plan usage limits are not available for this authentication method.")
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
            );
        }
        None if loading => {
            card = card.child(card_divider()).child(
                view().px(px(16.0)).py(px(11.0)).child(
                    text(format!(
                        "Checking remaining usage from {} CLI...",
                        provider.label()
                    ))
                    .text_size(px(12.0))
                    .text_color(palette::text_muted()),
                ),
            );
        }
        None => {
            let msg = warning.unwrap_or_else(|| {
                "Usage limits have not been loaded yet. Click Refresh to check.".to_owned()
            });
            card = card.child(card_divider()).child(
                view().px(px(16.0)).py(px(11.0)).child(
                    text(msg)
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
            );
        }
    }

    card
}

fn cli_usage_window_row(window: &ClaudeUsageWindow) -> impl IntoElement {
    let remaining_fraction = (window.remaining_percent as f32 / 100.0).clamp(0.0, 1.0);
    let tone_color = if window.remaining_percent <= 10 || window.severity == "critical" {
        palette::destructive_text()
    } else if window.remaining_percent <= 25 || window.severity == "warning" {
        palette::warning_text()
    } else {
        palette::success_text()
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
                    text(window.label.clone())
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            text(format!("{}% remaining", window.remaining_percent))
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .tabular_nums()
                                .text_color(tone_color),
                        )
                        .child(
                            text(format!("{}% used", window.used_percent))
                                .text_size(px(11.5))
                                .tabular_nums()
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(
            view()
                .w_full()
                .h(px(6.0))
                .rounded_full()
                .bg(palette::card_inset_bg())
                .overflow_hidden()
                .flex_row()
                .child(
                    view()
                        .h_full()
                        .w(relative(remaining_fraction))
                        .rounded_full()
                        .bg(tone_color),
                ),
        )
}

fn cli_quota_resets_section(resets: &[ClaudeQuotaReset]) -> impl IntoElement {
    let mut rows = view().gap(px(5.0));
    for item in resets {
        let formatted =
            format_reset_schedule_now(&item.resets_at).unwrap_or_else(|| item.resets_at.clone());
        rows = rows.child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    text(item.label.clone())
                        .text_size(px(12.0))
                        .text_color(palette::text_secondary()),
                )
                .child(
                    text(formatted)
                        .text_size(px(12.0))
                        .tabular_nums()
                        .text_color(palette::text_muted()),
                ),
        );
    }

    view()
        .px(px(16.0))
        .py(px(10.0))
        .gap(px(6.0))
        .child(
            text("Quota resets")
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_secondary()),
        )
        .child(rows)
}

fn cli_model_family_filter_row(
    provider: ProviderKind,
    families: Vec<(String, bool)>,
) -> impl IntoElement {
    let is_claude = provider == ProviderKind::Claude;
    let provider_slug = if is_claude { "claude" } else { "codex" };
    let mut checkboxes = view().flex_row().flex_wrap().gap(px(8.0));
    for (family, checked) in families {
        let family_for_press = family.clone();
        let test_id = format!(
            "prefs-{provider_slug}-family-{}",
            family.to_ascii_lowercase()
        );
        let label_for_a11y = format!("{family} model family");
        checkboxes = checkboxes.child(
            button()
                .test_id(&test_id)
                .accessibility_label(label_for_a11y)
                .on_press(move || {
                    if is_claude {
                        store::toggle_claude_model_family(&family_for_press);
                    } else {
                        store::toggle_codex_model_family(&family_for_press);
                    }
                })
                .px(px(9.0))
                .py(px(5.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(if checked {
                    palette::card_selected_border()
                } else {
                    palette::border_color()
                })
                .bg(if checked {
                    palette::card_selected_bg()
                } else {
                    palette::control_bg()
                })
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            view()
                                .w(px(14.0))
                                .h(px(14.0))
                                .rounded(px(3.5))
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
                                .items_center()
                                .justify_center()
                                .child(when(
                                    move || checked,
                                    || icons::check(palette::nav_active_fg()),
                                )),
                        )
                        .child(
                            text(family)
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if checked {
                                    palette::text_primary()
                                } else {
                                    palette::text_muted()
                                }),
                        ),
                ),
        );
    }

    view()
        .px(px(16.0))
        .py(px(11.0))
        .gap(px(8.0))
        .child(
            view()
                .gap(px(2.0))
                .child(
                    text("Model Family Filter")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("Uncheck a model family to hide it from the model selection view.")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(checkboxes)
}

fn cli_dynamic_model_picker(
    provider: ProviderKind,
    draft_model: Signal<String>,
    models: Vec<ClaudeModelOption>,
    total_count: usize,
    hidden_families: Vec<String>,
    active_editor: Signal<Option<ProviderEditor>>,
) -> impl IntoElement {
    let provider_label = provider.label();
    let provider_slug = if provider == ProviderKind::Claude {
        "claude"
    } else {
        "codex"
    };
    let visible_count = models.len();
    let caption = if hidden_families.is_empty() {
        format!("Available from {provider_label} CLI ({total_count} models)")
    } else {
        format!(
            "Showing {visible_count} of {total_count} models from {provider_label} CLI ({} hidden)",
            hidden_families.join(", ")
        )
    };
    let mut list = view().gap(px(4.0));
    if models.is_empty() {
        list = list.child(
            text("All model families are hidden by your Model Family Filter. Check a family below to show models.")
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    }
    for option in models {
        let value = option.value.clone();
        let value_for_select = value.clone();
        let value_for_check = value.clone();
        let resolved_for_check = option.resolved_model.clone();
        let meta_label = if option.value != option.resolved_model {
            format!("{} · {}", option.value, option.resolved_model)
        } else {
            option.value.clone()
        };
        let description = option.description.clone();
        let has_description = !description.is_empty();
        let test_id = format!("prefs-{provider_slug}-model-{}", option.value);

        list = list.child(
            button()
                .test_id(&test_id)
                .on_press(move || draft_model.set(value_for_select.clone()))
                .w_full()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(move || {
                    let current = draft_model.get();
                    if current == value_for_check || current == resolved_for_check {
                        palette::card_selected_border()
                    } else {
                        palette::border_color()
                    }
                })
                .bg(move || {
                    let current = draft_model.get();
                    if current == value || current == option.resolved_model {
                        palette::card_selected_bg()
                    } else {
                        palette::card_inset_bg()
                    }
                })
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    view()
                        .w_full()
                        .gap(px(2.0))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(8.0))
                                .child(
                                    text(option.display_name.clone())
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::text_primary()),
                                )
                                .child(
                                    text(meta_label)
                                        .app_class("font-mono")
                                        .text_size(px(11.0))
                                        .text_color(palette::text_muted()),
                                ),
                        )
                        .child(when(
                            move || has_description,
                            move || {
                                text(description.clone())
                                    .text_size(px(11.5))
                                    .text_color(palette::text_muted())
                            },
                        )),
                ),
        );
    }

    view()
        .pt(px(4.0))
        .gap(px(8.0))
        .child(
            text(caption)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(list)
        .child(inline_single_field_editor(
            draft_model,
            "Model alias or full model ID",
            true,
            "Save Model",
            move || {
                if store::update_selected_provider_model(&draft_model.get_untracked()) {
                    active_editor.set(None);
                }
            },
            move || {
                store::clear_preferences_messages();
                active_editor.set(None);
            },
        ))
}

fn cli_path_editor(
    provider: ProviderKind,
    draft_path: Signal<String>,
    has_custom_path: bool,
    active_editor: Signal<Option<ProviderEditor>>,
) -> impl IntoElement {
    let is_claude = provider == ProviderKind::Claude;
    let provider_slug = if is_claude { "claude" } else { "codex" };
    let bin_name = if is_claude { "claude" } else { "codex" };
    let dialog_title = format!("Select {} CLI Binary", provider.label());
    let helper_copy = format!(
        "Leave blank to auto-detect '{bin_name}' on the default PATH, or enter the full path to the executable."
    );
    let placeholder = format!("/usr/local/bin/{bin_name} or ~/.local/bin/{bin_name}");
    let input_test_id = format!("prefs-{provider_slug}-path-input");
    let browse_test_id = format!("prefs-{provider_slug}-path-browse");
    let reset_test_id = format!("prefs-{provider_slug}-path-reset");
    let save_test_id = format!("prefs-{provider_slug}-path-save");

    let save_path = move || {
        let ok = if is_claude {
            store::update_claude_cli_path(&draft_path.get_untracked())
        } else {
            store::update_codex_cli_path(&draft_path.get_untracked())
        };
        if ok {
            active_editor.set(None);
        }
    };
    let on_submit = save_path;

    let browse_binary = move || {
        let answer = pick_paths(
            OpenDialog::files()
                .title(dialog_title.clone())
                .button_label("Select"),
        );
        Owner::detached().run(|| {
            spawn(async move {
                if let Ok(Some(paths)) = answer.await
                    && let Some(chosen) = paths.into_iter().next()
                {
                    draft_path.set(chosen.display().to_string());
                }
            });
        });
    };

    view()
        .pt(px(4.0))
        .gap(px(8.0))
        .child(
            text(helper_copy)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    input()
                        .test_id(&input_test_id)
                        .value(move || draft_path.get())
                        .on_change(move |v: &str| draft_path.set(v.to_owned()))
                        .on_submit(move |_: &str| on_submit())
                        .placeholder(placeholder)
                        .app_class("font-mono")
                        .flex_1()
                        .px(px(10.0))
                        .py(px(6.5))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_inset_bg())
                        .rounded(px(7.0))
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .test_id(&browse_test_id)
                        .on_press(browse_binary)
                        .px(px(12.0))
                        .py(px(5.5))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Browse...")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(when(
                    move || has_custom_path,
                    move || {
                        button()
                            .test_id(&reset_test_id)
                            .on_press(move || {
                                if is_claude {
                                    store::reset_claude_cli_path();
                                } else {
                                    store::reset_codex_cli_path();
                                }
                                active_editor.set(None);
                            })
                            .px(px(10.0))
                            .py(px(5.5))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::control_bg())
                            .hover(|s| s.bg(palette::subtle_surface_bg()))
                            .active(|s| s.opacity(0.8))
                            .child(
                                text("Use Default PATH")
                                    .text_size(px(12.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_secondary()),
                            )
                    },
                ))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(secondary_button("Cancel", move || {
                            store::clear_preferences_messages();
                            active_editor.set(None);
                        }))
                        .child(
                            button()
                                .test_id(&save_test_id)
                                .on_press(save_path)
                                .px(px(12.0))
                                .py(px(6.0))
                                .rounded(px(7.0))
                                .bg(palette::nav_active_bg())
                                .active(|s| s.opacity(0.8))
                                .child(
                                    text("Save Path")
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::nav_active_fg()),
                                ),
                        ),
                ),
        )
}

fn detail_header(
    badge: &'static str,
    title: String,
    subtitle: String,
    test_state: &ConnectionTestState,
    ok_label: &'static str,
) -> impl IntoElement {
    let status_el: AnyElement = match test_state {
        ConnectionTestState::Works => view()
            .flex_row()
            .items_center()
            .gap(px(5.0))
            .child(icons::check(palette::success_text()))
            .child(
                text(ok_label)
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::success_text()),
            )
            .into_element(),
        ConnectionTestState::Failed(msg) => text(msg.clone())
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::destructive_text())
            .into_element(),
        ConnectionTestState::Untested => text("Not tested")
            .text_size(px(12.0))
            .text_color(palette::text_muted())
            .into_element(),
    };

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(detail_badge(badge))
                .child(
                    view()
                        .gap(px(1.5))
                        .child(
                            text(title)
                                .text_size(px(15.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(subtitle)
                                .text_size(px(12.0))
                                .text_color(palette::text_muted()),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(status_el)
                .child(secondary_button("Test", store::test_selected_provider)),
        )
}

fn detail_badge(label: &'static str) -> impl IntoElement {
    view()
        .w(px(36.0))
        .h(px(36.0))
        .rounded(px(8.0))
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .bg(palette::provider_badge_bg())
        .child(
            text(label)
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_secondary()),
        )
}

fn model_picker_editor(
    draft_model: Signal<String>,
    presets: &[&'static str],
    active_editor: Signal<Option<ProviderEditor>>,
) -> impl IntoElement {
    let mut chips = view().flex_row().flex_wrap().gap(px(6.0));
    for preset in presets {
        let preset_str = *preset;
        chips = chips.child(
            button()
                .on_press(move || draft_model.set(preset_str.to_owned()))
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text(preset_str)
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    view()
        .pt(px(4.0))
        .gap(px(8.0))
        .child(chips)
        .child(inline_single_field_editor(
            draft_model,
            "Model identifier",
            true,
            "Save Model",
            move || {
                if store::update_selected_provider_model(&draft_model.get_untracked()) {
                    active_editor.set(None);
                }
            },
            move || {
                store::clear_preferences_messages();
                active_editor.set(None);
            },
        ))
}

fn inline_single_field_editor(
    value: Signal<String>,
    placeholder: &'static str,
    monospace: bool,
    save_label: &'static str,
    on_save: impl Fn() + Clone + 'static,
    on_cancel: impl Fn() + 'static,
) -> impl IntoElement {
    let on_submit = on_save.clone();
    let mut field = input()
        .test_id("prefs-inline-input")
        .value(move || value.get())
        .on_change(move |v: &str| value.set(v.to_owned()))
        .on_submit(move |_: &str| on_submit())
        .placeholder(placeholder)
        .flex_1()
        .px(px(10.0))
        .py(px(6.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .rounded(px(7.0))
        .text_color(palette::text_primary());
    if monospace {
        field = field.app_class("font-mono");
    }

    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .pt(px(2.0))
        .child(field)
        .child(secondary_button("Cancel", on_cancel))
        .child(primary_button(save_label, on_save))
}

// --- General Tab ------------------------------------------------------------------------

fn general_tab() -> impl IntoElement {
    let projects_dir_draft = signal(store::preferences().general.default_projects_dir);

    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(22.0))
            .gap(px(16.0))
            .child(dynamic(move || {
                let prefs = store::preferences().general;
                let mut landing_picker = view()
                    .flex_row()
                    .items_center()
                    .p(px(2.5))
                    .rounded(px(8.0))
                    .bg(palette::segmented_track_bg())
                    .gap(px(2.0));
                for option in DefaultLandingView::ALL {
                    let active = prefs.default_landing_view == *option;
                    let target = *option;
                    landing_picker = landing_picker.child(segment_button(
                        option.label(),
                        active,
                        move || store::set_default_landing_view(target),
                    ));
                }

                view()
                    .gap(px(16.0))
                    .child(
                        grouped_card("Startup & Workspace")
                            .child(card_divider())
                            .child(setting_row(
                                "Default landing view",
                                "Screen shown when Bruk opens.",
                                landing_picker,
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Default projects directory",
                                "Starting folder when registering local repositories.",
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        input()
                                            .value(move || projects_dir_draft.get())
                                            .on_change(move |v: &str| {
                                                projects_dir_draft.set(v.to_owned())
                                            })
                                            .on_submit(move |v: &str| {
                                                store::set_default_projects_dir(v)
                                            })
                                            .w(px(180.0))
                                            .app_class("font-mono")
                                            .px(px(9.0))
                                            .py(px(6.0))
                                            .border_1()
                                            .border_color(palette::border_color())
                                            .bg(palette::card_inset_bg())
                                            .rounded(px(7.0))
                                            .text_color(palette::text_primary()),
                                    )
                                    .child(secondary_button("Apply", move || {
                                        store::set_default_projects_dir(
                                            &projects_dir_draft.get_untracked(),
                                        );
                                    })),
                            )),
                    )
                    .child(
                        grouped_card("Notifications & Safety")
                            .child(card_divider())
                            .child(toggle_row(
                                "Notify on human approval gates",
                                "Send a macOS notification when a workflow pauses for review.",
                                prefs.notify_on_human_gate,
                                || store::toggle_general_flag(GeneralFlag::NotifyHumanGate),
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Notify when a run fails",
                                "Alert immediately if a build, test, or agent step exits with an error.",
                                prefs.notify_on_run_failure,
                                || store::toggle_general_flag(GeneralFlag::NotifyRunFailure),
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Confirm before cancelling active runs",
                                "Prompt before terminating in-progress agent steps.",
                                prefs.confirm_before_cancel,
                                || store::toggle_general_flag(GeneralFlag::ConfirmBeforeCancel),
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Automatically check for updates",
                                "Periodically check for new Bruk releases.",
                                prefs.auto_check_updates,
                                || store::toggle_general_flag(GeneralFlag::AutoCheckUpdates),
                            )),
                    )
            })),
    )
}

// --- Appearance Tab ---------------------------------------------------------------------

fn appearance_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(22.0))
            .gap(px(16.0))
            .child(dynamic(move || {
                let prefs = store::preferences().appearance;

                let mut mode_picker = view()
                    .flex_row()
                    .items_center()
                    .p(px(2.5))
                    .rounded(px(8.0))
                    .bg(palette::segmented_track_bg())
                    .gap(px(2.0));
                for mode in AppearanceMode::ALL {
                    let active = prefs.mode == *mode;
                    let target = *mode;
                    mode_picker =
                        mode_picker.child(segment_button(mode.label(), active, move || {
                            store::set_appearance_mode(target)
                        }));
                }

                let mut size_picker = view()
                    .flex_row()
                    .items_center()
                    .p(px(2.5))
                    .rounded(px(8.0))
                    .bg(palette::segmented_track_bg())
                    .gap(px(2.0));
                for pt in [11u8, 12, 13, 14] {
                    let active = prefs.monospace_font_size == pt;
                    size_picker =
                        size_picker.child(segment_button(format!("{pt} pt"), active, move || {
                            store::set_monospace_font_size(pt)
                        }));
                }

                view()
                    .gap(px(16.0))
                    .child(
                        grouped_card("Theme")
                            .child(card_divider())
                            .child(setting_row(
                                "Appearance",
                                "Match macOS Light/Dark mode or lock to a single theme.",
                                mode_picker,
                            )),
                    )
                    .child(
                        grouped_card("Layout & Typography")
                            .child(card_divider())
                            .child(toggle_row(
                                "Compact navigation sidebar",
                                "Reduce row spacing in the left navigation rail.",
                                prefs.compact_sidebar,
                                store::toggle_compact_sidebar,
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Log & code font size",
                                "Monospace size used for step logs, commands, and API keys.",
                                size_picker,
                            )),
                    )
            })),
    )
}

// --- Machines Tab (matching 22 PreferencesMachines.png) ---------------------------------

fn machines_tab() -> impl IntoElement {
    let active_editor: Signal<Option<MachinePrefsEditor>> = signal(None);

    // Inline editor draft signals.
    let draft_text = signal(String::new());
    let draft_add_name = signal(String::new());
    let draft_add_ipv4 = signal(String::new());
    let draft_add_port = signal("22".to_owned());
    let draft_add_user = signal("bruk".to_owned());
    let draft_add_platform = signal(MachinePlatform::Linux);

    scroll().flex_1().bg(palette::canvas_bg()).child(
        view().flex_1().p(px(22.0)).child(
            view()
                .flex_row()
                .items_start()
                .gap(px(18.0))
                .child(machines_left_rail(
                    active_editor,
                    draft_add_name,
                    draft_add_ipv4,
                    draft_add_port,
                    draft_add_user,
                    draft_add_platform,
                ))
                .child(machines_detail_column(
                    active_editor,
                    draft_text,
                    draft_add_name,
                    draft_add_ipv4,
                    draft_add_port,
                    draft_add_user,
                    draft_add_platform,
                )),
        ),
    )
}

fn machines_left_rail(
    active_editor: Signal<Option<MachinePrefsEditor>>,
    draft_add_name: Signal<String>,
    draft_add_ipv4: Signal<String>,
    draft_add_port: Signal<String>,
    draft_add_user: Signal<String>,
    draft_add_platform: Signal<MachinePlatform>,
) -> impl IntoElement {
    view().w(px(222.0)).flex_shrink_0().child(dynamic(move || {
        let fleet = store::machines_fleet();
        let selected_id = store::selected_prefs_machine_id();
        let adding = active_editor.get() == Some(MachinePrefsEditor::AddMachine);

        let mut card = view()
            .rounded(px(10.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::card_bg())
            .p(px(8.0))
            .gap(px(3.0));

        if fleet.machines.is_empty() {
            card = card.child(
                text("No machines configured")
                    .px(px(6.0))
                    .py(px(6.0))
                    .text_size(px(12.0))
                    .text_color(palette::text_muted()),
            );
        } else {
            for machine in &fleet.machines {
                let is_selected = !adding && selected_id == Some(machine.id);
                card = card.child(machine_prefs_list_row(machine, is_selected, active_editor));
            }
        }

        card = card.child(
            view()
                .pt(px(8.0))
                .px(px(4.0))
                .pb(px(2.0))
                .items_start()
                .child(secondary_button_id(
                    "prefs-machine-add",
                    "Add Machine...",
                    move || {
                        store::clear_preferences_messages();
                        draft_add_name.set(String::new());
                        draft_add_ipv4.set(String::new());
                        draft_add_port.set("22".to_owned());
                        draft_add_user.set("bruk".to_owned());
                        draft_add_platform.set(MachinePlatform::Linux);
                        active_editor.set(Some(MachinePrefsEditor::AddMachine));
                    },
                )),
        );

        card
    }))
}

fn machine_prefs_list_row(
    machine: &MachineRecord,
    selected: bool,
    active_editor: Signal<Option<MachinePrefsEditor>>,
) -> impl IntoElement {
    let id = machine.id;
    let slug = slugify(&machine.name);
    let test_id = format!("prefs-machine-item-{slug}");

    let row_bg = if selected {
        palette::nav_active_bg()
    } else {
        AdaptiveColor::TRANSPARENT
    };
    let title_color = if selected {
        palette::nav_active_fg()
    } else {
        palette::text_primary()
    };
    let subtitle_color = if selected {
        AdaptiveColor::new(rgba(0xFFFFFFD9), rgba(0xFFFFFFD9))
    } else {
        palette::text_muted()
    };

    button()
        .test_id(&test_id)
        .accessibility_label(machine.name.clone())
        .on_press(move || {
            active_editor.set(None);
            store::select_prefs_machine(id);
        })
        .w_full()
        .rounded(px(8.0))
        .px(px(10.0))
        .py(px(7.5))
        .items_start()
        .gap(px(1.5))
        .bg(row_bg)
        .hover(move |s| {
            if selected {
                s
            } else {
                s.bg(palette::sidebar_hover_bg())
            }
        })
        .active(|s| s.opacity(0.85))
        .child(
            text(machine.name.clone())
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(title_color),
        )
        .child(
            text(machine.prefs_os_short.clone())
                .text_size(px(11.5))
                .text_color(subtitle_color),
        )
}

#[allow(clippy::too_many_arguments)]
fn machines_detail_column(
    active_editor: Signal<Option<MachinePrefsEditor>>,
    draft_text: Signal<String>,
    draft_add_name: Signal<String>,
    draft_add_ipv4: Signal<String>,
    draft_add_port: Signal<String>,
    draft_add_user: Signal<String>,
    draft_add_platform: Signal<MachinePlatform>,
) -> impl IntoElement {
    view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(14.0))
        .child(dynamic(move || {
            let editor = active_editor.get();
            if editor == Some(MachinePrefsEditor::AddMachine) {
                return add_machine_prefs_card(
                    active_editor,
                    draft_add_name,
                    draft_add_ipv4,
                    draft_add_port,
                    draft_add_user,
                    draft_add_platform,
                )
                .into_element();
            }

            let fleet = store::machines_fleet();
            if let Some(machine) = store::selected_prefs_machine() {
                machine_prefs_detail_pane(machine, fleet.bruk_schema, active_editor, draft_text)
                    .into_element()
            } else {
                grouped_card("Machines")
                    .child(card_divider())
                    .child(
                        view().p(px(16.0)).child(
                            text(
                                "No machines configured yet. Add a remote SSH host to get started.",
                            )
                            .text_size(px(12.5))
                            .text_color(palette::text_muted()),
                        ),
                    )
                    .into_element()
            }
        }))
}

fn add_machine_prefs_card(
    active_editor: Signal<Option<MachinePrefsEditor>>,
    draft_name: Signal<String>,
    draft_ipv4: Signal<String>,
    draft_port: Signal<String>,
    draft_user: Signal<String>,
    draft_platform: Signal<MachinePlatform>,
) -> impl IntoElement {
    let platform = draft_platform.get();
    let submit = move || {
        let name = draft_name.get_untracked();
        let ipv4 = draft_ipv4.get_untracked();
        let port = draft_port.get_untracked();
        let user = draft_user.get_untracked();
        let selected_platform = draft_platform.get_untracked();
        if store::add_prefs_machine(&name, &ipv4, &port, &user, selected_platform) {
            active_editor.set(None);
        }
    };

    view()
        .gap(px(12.0))
        .child(
            view().gap(px(1.5)).child(
                text("Add machine")
                    .text_size(px(16.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(palette::text_primary()),
            ),
        )
        .child(
            grouped_card("SSH connection").child(card_divider()).child(
                view()
                    .p(px(16.0))
                    .gap(px(12.0))
                    .child(form_field(
                        "Machine name",
                        input()
                            .test_id("prefs-new-machine-name")
                            .value(move || draft_name.get())
                            .on_change(move |v: &str| draft_name.set(v.to_owned()))
                            .placeholder("e.g. build-box-02")
                            .app_class("font-mono")
                            .px(px(10.0))
                            .py(px(7.0))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_inset_bg())
                            .rounded(px(7.0))
                            .text_color(palette::text_primary()),
                    ))
                    .child(
                        view()
                            .flex_row()
                            .gap(px(12.0))
                            .child(
                                view().flex_1().child(form_field(
                                    "IPv4 or hostname",
                                    input()
                                        .test_id("prefs-new-machine-ipv4")
                                        .value(move || draft_ipv4.get())
                                        .on_change(move |v: &str| draft_ipv4.set(v.to_owned()))
                                        .placeholder("10.0.4.55")
                                        .app_class("font-mono")
                                        .px(px(10.0))
                                        .py(px(7.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::card_inset_bg())
                                        .rounded(px(7.0))
                                        .text_color(palette::text_primary()),
                                )),
                            )
                            .child(
                                view().w(px(96.0)).child(form_field(
                                    "Port",
                                    input()
                                        .test_id("prefs-new-machine-port")
                                        .value(move || draft_port.get())
                                        .on_change(move |v: &str| draft_port.set(v.to_owned()))
                                        .placeholder("22")
                                        .app_class("font-mono")
                                        .px(px(10.0))
                                        .py(px(7.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::card_inset_bg())
                                        .rounded(px(7.0))
                                        .text_color(palette::text_primary()),
                                )),
                            )
                            .child(
                                view().flex_1().child(form_field(
                                    "SSH user",
                                    input()
                                        .test_id("prefs-new-machine-user")
                                        .value(move || draft_user.get())
                                        .on_change(move |v: &str| draft_user.set(v.to_owned()))
                                        .placeholder("bruk")
                                        .app_class("font-mono")
                                        .px(px(10.0))
                                        .py(px(7.0))
                                        .border_1()
                                        .border_color(palette::border_color())
                                        .bg(palette::card_inset_bg())
                                        .rounded(px(7.0))
                                        .text_color(palette::text_primary()),
                                )),
                            ),
                    )
                    .child(form_field(
                        "Platform",
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(machine_platform_option(
                                MachinePlatform::Linux,
                                platform,
                                draft_platform,
                            ))
                            .child(machine_platform_option(
                                MachinePlatform::Mac,
                                platform,
                                draft_platform,
                            ))
                            .child(machine_platform_option(
                                MachinePlatform::Windows,
                                platform,
                                draft_platform,
                            )),
                    ))
                    .child(
                        text(
                            "Bruk only connects with SSH keys from the macOS Keychain SSH agent. Password sign-in is not supported.",
                        )
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                    )
                    .child(preferences_error_banner())
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(secondary_button("Cancel", move || {
                                store::clear_preferences_messages();
                                active_editor.set(None);
                            }))
                            .child(
                                button()
                                    .test_id("prefs-new-machine-submit")
                                    .on_press(submit)
                                    .px(px(12.0))
                                    .py(px(6.0))
                                    .rounded(px(7.0))
                                    .bg(palette::nav_active_bg())
                                    .active(|s| s.opacity(0.8))
                                    .child(
                                        text("Add Machine")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::nav_active_fg()),
                                    ),
                            ),
                    ),
            ),
        )
}

fn machine_platform_option(
    platform: MachinePlatform,
    active: MachinePlatform,
    draft_platform: Signal<MachinePlatform>,
) -> impl IntoElement {
    let selected = platform == active;
    button()
        .test_id(&format!(
            "prefs-new-machine-platform-{}",
            platform.short_badge()
        ))
        .on_press(move || draft_platform.set(platform))
        .px(px(10.0))
        .py(px(5.0))
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
        .child(
            text(platform.display_name())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if selected {
                    palette::link_blue()
                } else {
                    palette::text_secondary()
                }),
        )
}

fn machine_prefs_detail_pane(
    machine: MachineRecord,
    bruk_schema: u32,
    active_editor: Signal<Option<MachinePrefsEditor>>,
    draft_text: Signal<String>,
) -> impl IntoElement {
    let id = machine.id;
    let is_local = machine.is_local;
    let editor = active_editor.get();

    let is_changing_user = editor == Some(MachinePrefsEditor::ChangeUser(id));
    let is_editing_jump_host = editor == Some(MachinePrefsEditor::EditJumpHost(id));
    let is_showing_host_key = editor == Some(MachinePrefsEditor::ShowHostKey(id));
    let is_changing_ssh_key = editor == Some(MachinePrefsEditor::ChangeSshKey(id));
    let is_changing_core_updates = editor == Some(MachinePrefsEditor::ChangeCoreUpdates(id));
    let is_confirming_claude_signout = editor
        == Some(MachinePrefsEditor::ConfirmAgentSignOut(
            id,
            MachineAgentKind::ClaudeCode,
        ));
    let is_confirming_codex_signout = editor
        == Some(MachinePrefsEditor::ConfirmAgentSignOut(
            id,
            MachineAgentKind::Codex,
        ));
    let is_changing_sessions = editor == Some(MachinePrefsEditor::ChangeSessionsAtOnce(id));
    let is_editing_projects = editor == Some(MachinePrefsEditor::EditProjects(id));
    let is_confirming_remove = editor == Some(MachinePrefsEditor::ConfirmRemove(id));

    let status_color = match machine.status {
        MachineConnectionStatus::Connected => palette::success_text(),
        MachineConnectionStatus::Disconnected => palette::workflow_now_amber(),
        MachineConnectionStatus::NotSyncing | MachineConnectionStatus::Unreachable => {
            palette::destructive_text()
        }
    };

    let schema_color = match machine.compatibility(bruk_schema) {
        SchemaCompatibility::InSync => {
            if machine.status.is_offline() {
                palette::text_muted()
            } else {
                palette::success_text()
            }
        }
        SchemaCompatibility::CoreUpdateSuggested => palette::pill_warning_text(),
        SchemaCompatibility::NeedsNewerBruk => palette::destructive_text(),
    };

    let current_user = machine.ssh_user.clone();
    let current_jump = machine.jump_host.clone().unwrap_or_default();
    let has_jump = machine.jump_host.is_some();
    let current_ssh_key = machine.ssh_key_name.clone();
    let current_sessions = machine.max_sessions_at_once.to_string();
    let machine_name_for_claude = machine.name.clone();
    let machine_name_for_codex = machine.name.clone();
    let machine_name_for_remove = machine.name.clone();
    let host_key_full = machine.host_key_full.clone();
    let host_key_type = machine.host_key_type.clone();
    let host_key_fp = machine.host_key_fingerprint.clone();
    let host_key_trusted = machine.host_key_trusted_on.clone();
    let current_policy = machine.core_update_policy;
    let allowed_projects = machine.allowed_projects.clone();
    let claude_signed_in = machine.claude_signed_in;
    let codex_signed_in = machine.codex_signed_in;

    // --- System card ---
    let system_card = grouped_card("System")
        .child(card_divider())
        .child(static_row(
            "Operating system",
            machine.os_detail.clone(),
            false,
        ))
        .child(card_divider())
        .child(static_row("Kernel", machine.kernel.clone(), true))
        .child(card_divider())
        .child(static_row(
            "Architecture",
            machine.architecture.clone(),
            true,
        ))
        .child(card_divider())
        .child(static_row(
            "Hardware",
            machine.hardware_memory.clone(),
            false,
        ))
        .child(card_divider())
        .child(static_row("Hostname", machine.hostname.clone(), true));

    // --- Access card ---
    let port_display = if is_local {
        "Local socket".to_owned()
    } else {
        machine.ssh_port.to_string()
    };

    let user_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("User"))
                        .child(
                            text(machine.ssh_user.clone())
                                .app_class("font-mono")
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(when(
                    move || !is_local,
                    move || {
                        let user_val = current_user.clone();
                        secondary_button_id("prefs-machine-change-user", "Change...", move || {
                            store::clear_preferences_messages();
                            draft_text.set(user_val.clone());
                            active_editor.set(Some(MachinePrefsEditor::ChangeUser(id)));
                        })
                    },
                )),
        )
        .child(when(
            move || is_changing_user,
            move || {
                inline_single_field_editor(
                    draft_text,
                    "SSH username",
                    true,
                    "Save User",
                    move || {
                        if store::update_prefs_machine_user(id, &draft_text.get_untracked()) {
                            active_editor.set(None);
                        }
                    },
                    move || {
                        store::clear_preferences_messages();
                        active_editor.set(None);
                    },
                )
            },
        ));

    let jump_value_el = {
        let mut t = text(
            machine
                .jump_host
                .clone()
                .unwrap_or_else(|| "None".to_owned()),
        )
        .text_size(px(12.5))
        .text_color(palette::text_primary());
        if has_jump {
            t = t.app_class("font-mono");
        }
        t
    };

    let jump_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Jump host"))
                        .child(jump_value_el),
                )
                .child(when(
                    move || !is_local,
                    move || {
                        let jump_val = current_jump.clone();
                        let btn_label = if has_jump { "Change..." } else { "Add..." };
                        secondary_button_id("prefs-machine-edit-jump-host", btn_label, move || {
                            store::clear_preferences_messages();
                            draft_text.set(jump_val.clone());
                            active_editor.set(Some(MachinePrefsEditor::EditJumpHost(id)));
                        })
                    },
                )),
        )
        .child(when(
            move || is_editing_jump_host,
            move || {
                inline_single_field_editor(
                    draft_text,
                    "bastion.example.com (blank for None)",
                    true,
                    "Save Jump Host",
                    move || {
                        if store::update_prefs_machine_jump_host(id, &draft_text.get_untracked()) {
                            active_editor.set(None);
                        }
                    },
                    move || {
                        store::clear_preferences_messages();
                        active_editor.set(None);
                    },
                )
            },
        ));

    let host_key_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Host key"))
                        .child(
                            view().flex_1().min_w(px(0.0)).child(
                                text(machine.host_key_summary())
                                    .app_class("font-mono")
                                    .text_size(px(12.0))
                                    .text_color(palette::text_primary()),
                            ),
                        ),
                )
                .child(when(
                    move || !is_local,
                    move || {
                        let btn_label = if is_showing_host_key { "Hide" } else { "Show" };
                        secondary_button_id("prefs-machine-show-host-key", btn_label, move || {
                            if active_editor.get_untracked()
                                == Some(MachinePrefsEditor::ShowHostKey(id))
                            {
                                active_editor.set(None);
                            } else {
                                active_editor.set(Some(MachinePrefsEditor::ShowHostKey(id)));
                            }
                        })
                    },
                )),
        )
        .child(when(
            move || is_showing_host_key,
            move || {
                view()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::card_inset_bg())
                    .p(px(10.0))
                    .gap(px(4.0))
                    .child(
                        text(format!(
                            "Trusted host key · {} · {} · verified {}",
                            host_key_type, host_key_fp, host_key_trusted
                        ))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_secondary()),
                    )
                    .child(
                        text(host_key_full.clone())
                            .app_class("font-mono")
                            .text_size(px(11.0))
                            .text_color(palette::text_muted()),
                    )
            },
        ));

    let access_card = grouped_card("Access")
        .child(card_divider())
        .child(static_row("IPv4", machine.ipv4.clone(), true))
        .child(card_divider())
        .child(static_row("IPv6", machine.ipv6.clone(), true))
        .child(card_divider())
        .child(static_row("Port", port_display, true))
        .child(card_divider())
        .child(user_row)
        .child(card_divider())
        .child(jump_row)
        .child(card_divider())
        .child(host_key_row);

    // --- SSH key card ---
    let ssh_key_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Key in use"))
                        .child(
                            text(machine.ssh_key_in_use_label())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(when(
                    move || !is_local,
                    move || {
                        let key_val = current_ssh_key.clone();
                        secondary_button_id(
                            "prefs-machine-change-ssh-key",
                            "Change...",
                            move || {
                                store::clear_preferences_messages();
                                draft_text.set(key_val.clone());
                                active_editor.set(Some(MachinePrefsEditor::ChangeSshKey(id)));
                            },
                        )
                    },
                )),
        )
        .child(when(
            move || is_changing_ssh_key,
            move || ssh_key_picker_editor(id, draft_text, active_editor),
        ));

    let ssh_key_card = grouped_card("SSH key")
        .child(card_divider())
        .child(ssh_key_row)
        .child(card_divider())
        .child(static_row(
            "Fingerprint",
            machine.ssh_key_short_fingerprint.clone(),
            true,
        ))
        .child(card_divider())
        .child(static_row(
            "Provided by",
            machine.ssh_key_provider.clone(),
            false,
        ))
        .child(
            view().px(px(16.0)).pt(px(2.0)).pb(px(12.0)).child(
                text("Bruk only connects with SSH keys. Password sign-in is not supported.")
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
            ),
        );

    // --- Bruk core card ---
    let data_schema_row = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(10.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(row_label("Data schema"))
                .child(
                    text(machine.data_schema_detail(bruk_schema))
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(schema_color),
                ),
        )
        .child(secondary_button_id(
            "prefs-machine-check-core",
            "Check Now",
            move || store::check_prefs_machine_core(id),
        ));

    let core_updates_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Core updates"))
                        .child(
                            text(machine.core_update_policy.label())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(secondary_button_id(
                    "prefs-machine-change-core-updates",
                    "Change...",
                    move || {
                        store::clear_preferences_messages();
                        if active_editor.get_untracked()
                            == Some(MachinePrefsEditor::ChangeCoreUpdates(id))
                        {
                            active_editor.set(None);
                        } else {
                            active_editor.set(Some(MachinePrefsEditor::ChangeCoreUpdates(id)));
                        }
                    },
                )),
        )
        .child(when(
            move || is_changing_core_updates,
            move || core_update_policy_picker(id, current_policy, active_editor),
        ));

    let bruk_core_card = grouped_card("Bruk core")
        .child(card_divider())
        .child(static_row("Installed", machine.core_version.clone(), true))
        .child(card_divider())
        .child(data_schema_row)
        .child(card_divider())
        .child(static_row(
            "Install path",
            machine.core_install_path.clone(),
            true,
        ))
        .child(card_divider())
        .child(core_updates_row)
        .child(
            view().px(px(16.0)).pt(px(2.0)).pb(px(12.0)).child(
                text(
                    "Versions only matter when the data schema differs. Bruk and the core compare schemas on every connection.",
                )
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
            ),
        );

    // --- Agents on this machine card ---
    let claude_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Claude Code"))
                        .child(
                            view().flex_1().min_w(px(0.0)).child(
                                text(machine.claude_status_summary())
                                    .text_size(px(12.5))
                                    .text_color(palette::text_primary()),
                            ),
                        ),
                )
                .child(secondary_button_id(
                    "prefs-machine-claude-auth",
                    if claude_signed_in {
                        "Sign Out..."
                    } else {
                        "Sign In..."
                    },
                    move || {
                        store::clear_preferences_messages();
                        if claude_signed_in {
                            active_editor.set(Some(MachinePrefsEditor::ConfirmAgentSignOut(
                                id,
                                MachineAgentKind::ClaudeCode,
                            )));
                        } else {
                            active_editor.set(None);
                            store::set_prefs_machine_agent_signed_in(
                                id,
                                MachineAgentKind::ClaudeCode,
                                true,
                            );
                        }
                    },
                )),
        )
        .child(when(
            move || is_confirming_claude_signout,
            move || {
                agent_signout_confirm_banner(
                    id,
                    MachineAgentKind::ClaudeCode,
                    machine_name_for_claude.clone(),
                    active_editor,
                )
            },
        ));

    let codex_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Codex"))
                        .child(
                            view().flex_1().min_w(px(0.0)).child(
                                text(machine.codex_status_summary())
                                    .text_size(px(12.5))
                                    .text_color(palette::text_primary()),
                            ),
                        ),
                )
                .child(secondary_button_id(
                    "prefs-machine-codex-auth",
                    if codex_signed_in {
                        "Sign Out..."
                    } else {
                        "Sign In..."
                    },
                    move || {
                        store::clear_preferences_messages();
                        if codex_signed_in {
                            active_editor.set(Some(MachinePrefsEditor::ConfirmAgentSignOut(
                                id,
                                MachineAgentKind::Codex,
                            )));
                        } else {
                            active_editor.set(None);
                            store::set_prefs_machine_agent_signed_in(
                                id,
                                MachineAgentKind::Codex,
                                true,
                            );
                        }
                    },
                )),
        )
        .child(when(
            move || is_confirming_codex_signout,
            move || {
                agent_signout_confirm_banner(
                    id,
                    MachineAgentKind::Codex,
                    machine_name_for_codex.clone(),
                    active_editor,
                )
            },
        ));

    let agents_card = grouped_card("Agents on this machine")
        .child(card_divider())
        .child(claude_row)
        .child(card_divider())
        .child(codex_row)
        .child(card_divider())
        .child(static_row(
            "OpenRouter",
            "The project's key is sent per session from this Mac's Keychain, never stored on the machine"
                .to_owned(),
            false,
        ));

    // --- Work card ---
    let sessions_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Sessions at once"))
                        .child(
                            text(machine.max_sessions_at_once.to_string())
                                .text_size(px(12.5))
                                .tabular_nums()
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(secondary_button_id(
                    "prefs-machine-change-sessions",
                    "Change...",
                    move || {
                        store::clear_preferences_messages();
                        draft_text.set(current_sessions.clone());
                        active_editor.set(Some(MachinePrefsEditor::ChangeSessionsAtOnce(id)));
                    },
                )),
        )
        .child(when(
            move || is_changing_sessions,
            move || sessions_limit_editor(id, draft_text, active_editor),
        ));

    let projects_row = view()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
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
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .child(row_label("Projects"))
                        .child(
                            view().flex_1().min_w(px(0.0)).child(
                                text(machine.allowed_projects_summary())
                                    .text_size(px(12.5))
                                    .text_color(palette::text_primary()),
                            ),
                        ),
                )
                .child(secondary_button_id(
                    "prefs-machine-edit-projects",
                    if is_editing_projects {
                        "Done"
                    } else {
                        "Edit..."
                    },
                    move || {
                        store::clear_preferences_messages();
                        if active_editor.get_untracked()
                            == Some(MachinePrefsEditor::EditProjects(id))
                        {
                            active_editor.set(None);
                        } else {
                            active_editor.set(Some(MachinePrefsEditor::EditProjects(id)));
                        }
                    },
                )),
        )
        .child(when(
            move || is_editing_projects,
            move || machine_projects_editor(id, allowed_projects.clone()),
        ));

    let work_card = grouped_card("Work")
        .child(card_divider())
        .child(static_row(
            "Workspace folder",
            machine.workspace_folder.clone(),
            true,
        ))
        .child(card_divider())
        .child(sessions_row)
        .child(card_divider())
        .child(projects_row);

    // --- Detail Header ---
    let header_row = view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            text(machine.name.clone())
                .text_size(px(16.0))
                .font_weight(FontWeight::BOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(machine.status.label())
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(status_color),
                )
                .child(secondary_button_id(
                    "prefs-machine-test-connection",
                    "Test Connection",
                    move || store::test_prefs_machine_connection(id),
                )),
        );

    let mut container = view()
        .gap(px(14.0))
        .child(header_row)
        .child(preferences_error_banner())
        .child(preferences_feedback_caption())
        .child(system_card)
        .child(access_card)
        .child(ssh_key_card)
        .child(bruk_core_card)
        .child(agents_card)
        .child(work_card);

    if !is_local {
        container = container
            .child(
                view().items_start().child(
                    button()
                        .test_id("prefs-machine-remove")
                        .accessibility_label("Remove Machine...")
                        .on_press(move || {
                            store::clear_preferences_messages();
                            active_editor.set(Some(MachinePrefsEditor::ConfirmRemove(id)));
                        })
                        .px(px(12.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Remove Machine...")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::destructive_text()),
                        ),
                ),
            )
            .child(when(
                move || is_confirming_remove,
                move || {
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .p(px(12.0))
                        .rounded(px(9.0))
                        .border_1()
                        .border_color(palette::destructive_text())
                        .bg(palette::card_bg())
                        .child(
                            text(format!(
                                "Remove '{}' from Bruk? Sessions on the host will keep running.",
                                machine_name_for_remove
                            ))
                            .text_size(px(12.5))
                            .text_color(palette::text_primary()),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(secondary_button("Cancel", move || {
                                    active_editor.set(None);
                                }))
                                .child(
                                    button()
                                        .test_id("prefs-machine-remove-confirm")
                                        .on_press(move || {
                                            if store::remove_prefs_machine(id) {
                                                active_editor.set(None);
                                            }
                                        })
                                        .px(px(12.0))
                                        .py(px(6.0))
                                        .rounded(px(7.0))
                                        .bg(palette::destructive_text())
                                        .active(|s| s.opacity(0.8))
                                        .child(
                                            text("Remove Machine")
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(palette::nav_active_fg()),
                                        ),
                                ),
                        )
                },
            ));
    } else {
        container = container.child(
            text("This Mac is the local machine and cannot be removed.")
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    }

    container
}

fn ssh_key_picker_editor(
    machine_id: u64,
    draft_key: Signal<String>,
    active_editor: Signal<Option<MachinePrefsEditor>>,
) -> impl IntoElement {
    const KEY_PRESETS: &[(&str, &str, &str)] = &[
        ("bruk-build", "ED25519", "SHA256:q3Ff…8kPw"),
        ("bruk-ci", "ED25519", "SHA256:k8Lm…2pRt"),
        ("bruk-deploy", "ED25519", "SHA256:z8Kp…1mQr"),
        ("id_ed25519_work", "ED25519", "SHA256:p4Tn…6wVx"),
    ];

    let mut chips = view().flex_row().flex_wrap().gap(px(6.0));
    for (name, key_type, fp) in KEY_PRESETS {
        let name_str = *name;
        let type_str = *key_type;
        let fp_str = *fp;
        chips = chips.child(
            button()
                .test_id(&format!("prefs-machine-ssh-preset-{name_str}"))
                .on_press(move || {
                    if store::update_prefs_machine_ssh_key(machine_id, name_str, type_str, fp_str) {
                        active_editor.set(None);
                    }
                })
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text(format!("{name_str} · {type_str}"))
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    view()
        .pt(px(4.0))
        .gap(px(8.0))
        .child(chips)
        .child(inline_single_field_editor(
            draft_key,
            "SSH key label in macOS Keychain",
            true,
            "Save Key",
            move || {
                if store::update_prefs_machine_ssh_key(
                    machine_id,
                    &draft_key.get_untracked(),
                    "ED25519",
                    "SHA256:v7Rm…3kLq",
                ) {
                    active_editor.set(None);
                }
            },
            move || {
                store::clear_preferences_messages();
                active_editor.set(None);
            },
        ))
}

fn core_update_policy_picker(
    machine_id: u64,
    current_policy: CoreUpdatePolicy,
    active_editor: Signal<Option<MachinePrefsEditor>>,
) -> impl IntoElement {
    let mut options = view().pt(px(4.0)).gap(px(6.0));
    for policy in CoreUpdatePolicy::ALL {
        let policy_val = *policy;
        let selected = policy_val == current_policy;
        let test_id = match policy_val {
            CoreUpdatePolicy::SuggestAskFirst => "prefs-machine-core-policy-suggest",
            CoreUpdatePolicy::AutoWhenBehind => "prefs-machine-core-policy-auto",
            CoreUpdatePolicy::ManualOnly => "prefs-machine-core-policy-manual",
        };
        options = options.child(
            button()
                .test_id(test_id)
                .on_press(move || {
                    store::set_prefs_machine_core_update_policy(machine_id, policy_val);
                    active_editor.set(None);
                })
                .w_full()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(if selected {
                    palette::card_selected_border()
                } else {
                    palette::border_color()
                })
                .bg(if selected {
                    palette::card_selected_bg()
                } else {
                    palette::card_inset_bg()
                })
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(policy_val.label())
                        .text_size(px(12.0))
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if selected {
                            palette::link_blue()
                        } else {
                            palette::text_primary()
                        }),
                )
                .child(when(
                    move || selected,
                    || icons::check(palette::link_blue()),
                )),
        );
    }
    options
}

fn agent_signout_confirm_banner(
    machine_id: u64,
    agent: MachineAgentKind,
    machine_name: String,
    active_editor: Signal<Option<MachinePrefsEditor>>,
) -> impl IntoElement {
    let confirm_id = match agent {
        MachineAgentKind::ClaudeCode => "prefs-machine-claude-signout-confirm",
        MachineAgentKind::Codex => "prefs-machine-codex-signout-confirm",
    };
    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(10.0))
        .p(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .child(
            text(format!(
                "Sign out of {} in Bruk's isolated config on {}?",
                agent.label(),
                machine_name
            ))
            .text_size(px(12.0))
            .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(secondary_button("Cancel", move || {
                    active_editor.set(None);
                }))
                .child(
                    button()
                        .test_id(confirm_id)
                        .on_press(move || {
                            store::set_prefs_machine_agent_signed_in(machine_id, agent, false);
                            active_editor.set(None);
                        })
                        .px(px(11.0))
                        .py(px(5.5))
                        .rounded(px(7.0))
                        .bg(palette::destructive_text())
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Sign Out")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::nav_active_fg()),
                        ),
                ),
        )
}

fn sessions_limit_editor(
    machine_id: u64,
    draft_sessions: Signal<String>,
    active_editor: Signal<Option<MachinePrefsEditor>>,
) -> impl IntoElement {
    let mut chips = view().flex_row().flex_wrap().gap(px(6.0));
    for count in [1u32, 2, 4, 6, 8] {
        let count_str = count.to_string();
        chips = chips.child(
            button()
                .test_id(&format!("prefs-machine-sessions-preset-{count}"))
                .on_press(move || {
                    if store::update_prefs_machine_max_sessions(machine_id, &count_str) {
                        active_editor.set(None);
                    }
                })
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    text(count.to_string())
                        .text_size(px(12.0))
                        .tabular_nums()
                        .text_color(palette::text_secondary()),
                ),
        );
    }

    view()
        .pt(px(4.0))
        .gap(px(8.0))
        .child(chips)
        .child(inline_single_field_editor(
            draft_sessions,
            "1 to 32",
            true,
            "Save Limit",
            move || {
                if store::update_prefs_machine_max_sessions(
                    machine_id,
                    &draft_sessions.get_untracked(),
                ) {
                    active_editor.set(None);
                }
            },
            move || {
                store::clear_preferences_messages();
                active_editor.set(None);
            },
        ))
}

fn machine_projects_editor(machine_id: u64, allowed_projects: Vec<String>) -> impl IntoElement {
    let available = store::available_machine_project_names();
    let mut chips = view().flex_row().flex_wrap().gap(px(8.0)).pt(px(4.0));
    for project in available {
        let allowed = allowed_projects.iter().any(|p| p == &project);
        let slug = slugify(&project);
        let project_for_click = project.clone();
        chips = chips.child(
            button()
                .test_id(&format!("prefs-machine-project-{slug}"))
                .accessibility_label(format!("Toggle project {project}"))
                .on_press(move || {
                    store::toggle_prefs_machine_project(machine_id, &project_for_click)
                })
                .px(px(10.0))
                .py(px(5.0))
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
    chips
}

fn secondary_button_id(
    test_id: &str,
    label: &'static str,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .on_press(on_press)
        .flex_shrink_0()
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

fn slugify(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "-")
}

// --- Extensions Tab ---------------------------------------------------------------------

fn extensions_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(22.0))
            .gap(px(16.0))
            .child(dynamic(move || {
                let prefs = store::preferences().extensions;
                let mut list_card = grouped_card("Installed Extensions");
                if prefs.entries.is_empty() {
                    list_card = list_card.child(card_divider()).child(
                        view().p(px(16.0)).gap(px(4.0)).child(
                            text("No extensions installed")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text("Extensions that add issue sources, workflow steps, or status widgets will appear here.")
                                .text_size(px(12.0))
                                .text_color(palette::text_muted()),
                        ),
                    );
                } else {
                    for entry in prefs.entries {
                        let entry_id = entry.id.clone();
                        let label = format!("{}  v{}", entry.name, entry.version);
                        list_card = list_card.child(card_divider()).child(toggle_row(
                            label,
                            entry.description,
                            entry.enabled,
                            move || store::toggle_extension(&entry_id),
                        ));
                    }
                }

                view()
                    .gap(px(16.0))
                    .child(list_card)
                    .child(
                        grouped_card("Project MCP Tools")
                            .child(card_divider())
                            .child(toggle_row(
                                "Allow project-defined MCP servers",
                                "Let repositories declare workspace-scoped MCP tool servers in their config.",
                                prefs.allow_project_mcp_servers,
                                store::toggle_allow_project_mcp_servers,
                            )),
                    )
            })),
    )
}

// --- Advanced Tab -----------------------------------------------------------------------

fn advanced_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(22.0))
            .gap(px(16.0))
            .child(dynamic(move || {
                let prefs = store::preferences().advanced;
                let db_path = store::db_path_display();

                view()
                    .gap(px(16.0))
                    .child(
                        grouped_card("Storage & Isolation")
                            .child(card_divider())
                            .child(static_row("SQLite database", db_path, true))
                            .child(card_divider())
                            .child(static_row(
                                "Provider CLI configs",
                                prefs.isolated_cli_root.clone(),
                                true,
                            )),
                    )
                    .child(
                        grouped_card("Diagnostics & Worktrees")
                            .child(card_divider())
                            .child(toggle_row(
                                "Verbose step logs",
                                "Capture full tool call payloads and stdout streams for every step.",
                                prefs.verbose_step_logs,
                                store::toggle_verbose_step_logs,
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Keep failed git worktrees",
                                "Preserve worktree folders after failed runs for manual inspection.",
                                prefs.keep_worktrees_on_failure,
                                store::toggle_keep_worktrees_on_failure,
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Log retention",
                                "Days to keep archived run logs in local storage.",
                                stepper_control(
                                    format!("{} days", prefs.log_retention_days),
                                    "Decrease log retention",
                                    "Increase log retention",
                                    || store::step_log_retention_days(-7),
                                    || store::step_log_retention_days(7),
                                ),
                            )),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(preferences_feedback_caption())
                            .child(
                                button()
                                    .on_press(store::reset_preferences_to_defaults)
                                    .px(px(12.0))
                                    .py(px(6.0))
                                    .rounded(px(7.0))
                                    .border_1()
                                    .border_color(palette::border_color())
                                    .bg(palette::control_bg())
                                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                                    .active(|s| s.opacity(0.8))
                                    .child(
                                        text("Reset Preferences to Defaults")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(palette::destructive_text()),
                                    ),
                            ),
                    )
            })),
    )
}

// --- Shared macOS Settings Primitives ---------------------------------------------------

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

fn row_label(label: &str) -> impl IntoElement {
    view().w(px(148.0)).flex_shrink_0().child(
        text(label.to_owned())
            .text_size(px(13.0))
            .text_color(palette::text_muted()),
    )
}

fn static_row(label: &str, value: String, monospace: bool) -> impl IntoElement {
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
        .child(row_label(label))
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

fn segment_button(
    label: impl Into<String>,
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
            text(label.into())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if active {
                    palette::text_primary()
                } else {
                    palette::text_muted()
                }),
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
                .min_w(px(58.0))
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

fn form_field(label: &'static str, control: impl IntoElement) -> impl IntoElement {
    view()
        .gap(px(5.0))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
        .child(control)
}

fn secondary_button(label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
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

fn primary_button(label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
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

fn preferences_error_banner() -> impl IntoElement {
    when(
        || store::preferences_error().is_some(),
        || {
            text(move || store::preferences_error().unwrap_or_default())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::destructive_text())
        },
    )
}

fn preferences_feedback_caption() -> impl IntoElement {
    when(
        || store::preferences_feedback().is_some(),
        || {
            text(move || store::preferences_feedback().unwrap_or_default())
                .text_size(px(12.0))
                .text_color(palette::text_muted())
        },
    )
}
