//! Native macOS Preferences window (`⌘,`) with tabbed settings and provider account management.

use factory_core::{
    AppearanceMode, CliAccount, ConnectionTestState, DefaultLandingView, OpenRouterKey,
    PreferencesTab, ProviderKind, format_whole_or_cents,
};
use nsh_base::{button, input};
use nsh_core::{
    desktop::{TitleBarOptions, TitleBarStyle, WindowAttributes, WindowKind, WindowPosition},
    element::desktop::{current_window, window},
    prelude::*,
};

use crate::{
    store::{self, GeneralFlag},
    ui::{icons, palette},
};

/// Active inline editor inside the Providers detail pane.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProviderEditor {
    AddOpenRouterKey,
    ReplaceKey(String),
    EditCap(String),
    ChangeModel(String),
    RenameKey(String),
    ConfirmRemove(String),
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
                    )),
            )
            .child(
                text(
                    "Claude and Codex run through their CLIs, each with its own configuration folder, so your global settings, sign-ins and history are never read or changed. OpenRouter keys stay in the macOS Keychain. Workflow and agent files name a provider, never a key, so they stay portable; each project picks its OpenRouter key in Project Settings › Providers.",
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
                    selected_id == prefs.claude.id
                        && active_editor.get() != Some(ProviderEditor::AddOpenRouterKey),
                    active_editor,
                    add_menu_open,
                ));

            // Codex group
            card = card
                .child(provider_group_header(ProviderKind::Codex))
                .child(provider_list_row(
                    prefs.codex.id.clone(),
                    ProviderKind::Codex.badge(),
                    prefs.codex.display_name.clone(),
                    prefs.codex.list_subtitle(),
                    false,
                    selected_id == prefs.codex.id
                        && active_editor.get() != Some(ProviderEditor::AddOpenRouterKey),
                    active_editor,
                    add_menu_open,
                ));

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
                        selected_id == key.id
                            && active_editor.get() != Some(ProviderEditor::AddOpenRouterKey),
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
                            .child(menu_option_button("Manage Codex CLI account", move || {
                                add_menu_open.set(false);
                                active_editor.set(None);
                                store::select_provider("codex".to_owned());
                            }))
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

fn providers_detail_column(
    active_editor: Signal<Option<ProviderEditor>>,
    add_menu_open: Signal<bool>,
    draft_primary: Signal<String>,
    draft_secondary: Signal<String>,
    draft_cap: Signal<String>,
    draft_model: Signal<String>,
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
                cli_account_detail_pane(account, active_editor, add_menu_open, draft_model)
                    .into_element()
            } else {
                cli_account_detail_pane(prefs.claude, active_editor, add_menu_open, draft_model)
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
                            "Remove '{}' from Keychain and Software Factory?",
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
    draft_model: Signal<String>,
) -> impl IntoElement {
    let account_id = account.id.clone();
    let account_id_model = account.id.clone();
    let account_id_toggle = account.id.clone();
    let current_model = account.default_model.clone();
    let connected = account.connected;
    let is_changing_model =
        active_editor.get() == Some(ProviderEditor::ChangeModel(account.id.clone()));

    let model_presets: &[&str] = match account.provider {
        ProviderKind::Claude => &["claude-sonnet-4-6", "claude-opus-4-6", "claude-haiku-4-5"],
        ProviderKind::Codex => &["gpt-5.3-codex", "gpt-5.2-codex", "o4-mini"],
        ProviderKind::OpenRouter => &["openrouter/auto"],
    };

    let projects_display = if account.used_by_projects.is_empty() {
        "None".to_owned()
    } else {
        account.used_by_projects.join(" · ")
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
                .child(static_row(
                    "Account",
                    if connected {
                        account.email.clone()
                    } else {
                        "Not signed in".to_owned()
                    },
                    false,
                ))
                .child(card_divider())
                .child(static_row(
                    "Subscription",
                    if connected {
                        account.plan.clone()
                    } else {
                        "—".to_owned()
                    },
                    false,
                ))
                .child(card_divider())
                .child(static_row("CLI binary", account.cli_binary.clone(), true))
                .child(card_divider())
                .child(static_row(
                    "Config folder",
                    account.config_dir.clone(),
                    true,
                )),
        )
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
                                .child(secondary_button("Change...", move || {
                                    add_menu_open.set(false);
                                    store::clear_preferences_messages();
                                    draft_model.set(current_model.clone());
                                    active_editor.set(Some(ProviderEditor::ChangeModel(
                                        account_id_model.clone(),
                                    )));
                                })),
                        )
                        .child(when(
                            move || is_changing_model,
                            move || model_picker_editor(draft_model, model_presets, active_editor),
                        )),
                ),
        )
        .child(
            grouped_card("Used by")
                .child(card_divider())
                .child(static_row("Projects", projects_display, false)),
        )
        .child(preferences_error_banner())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(if connected {
                    button()
                        .on_press(move || {
                            store::set_cli_account_connected(&account_id_toggle, false)
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
                            text("Sign Out")
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::destructive_text()),
                        )
                        .into_element()
                } else {
                    primary_button("Sign In", move || {
                        store::set_cli_account_connected(&account_id, true)
                    })
                    .into_element()
                }),
        )
        .child(preferences_feedback_caption())
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
    presets: &'static [&'static str],
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
                                "Screen shown when Software Factory opens.",
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
                                "Periodically check for new Software Factory releases.",
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

// --- Machines Tab -----------------------------------------------------------------------

fn machines_tab() -> impl IntoElement {
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(22.0))
            .gap(px(16.0))
            .child(dynamic(move || {
                let prefs = store::preferences().machines;
                let machines = store::machines();
                let connected = store::connected_machine_count();
                let disconnected = store::disconnected_machine_count();

                view()
                    .gap(px(16.0))
                    .child(
                        grouped_card("Worker Scheduling")
                            .child(card_divider())
                            .child(setting_row(
                                "Max concurrent runs per machine",
                                "Upper bound on parallel agent runs scheduled on one host.",
                                stepper_control(
                                    format!("{}", prefs.max_concurrent_runs),
                                    "Decrease max concurrent runs",
                                    "Increase max concurrent runs",
                                    || store::step_max_concurrent_runs(-1),
                                    || store::step_max_concurrent_runs(1),
                                ),
                            ))
                            .child(card_divider())
                            .child(setting_row(
                                "Heartbeat interval",
                                "How often remote worker hosts report capacity and status.",
                                stepper_control(
                                    format!("{}s", prefs.heartbeat_interval_secs),
                                    "Decrease heartbeat interval",
                                    "Increase heartbeat interval",
                                    || store::step_heartbeat_interval(-5),
                                    || store::step_heartbeat_interval(5),
                                ),
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Prefer local Mac when slots are free",
                                "Schedule runs locally before spilling over to remote builders.",
                                prefs.prefer_local_machine,
                                store::toggle_prefer_local_machine,
                            ))
                            .child(card_divider())
                            .child(toggle_row(
                                "Automatically reconnect offline runners",
                                "Retry SSH/daemon connections when a remote host drops.",
                                prefs.auto_reconnect_runners,
                                store::toggle_auto_reconnect_runners,
                            )),
                    )
                    .child(
                        grouped_card("Configured Fleet")
                            .child(card_divider())
                            .child(static_row(
                                "Active workers",
                                format!(
                                    "{connected} connected · {disconnected} disconnected ({} total)",
                                    machines.len()
                                ),
                                false,
                            )),
                    )
            })),
    )
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
