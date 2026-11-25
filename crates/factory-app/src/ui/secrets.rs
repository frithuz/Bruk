//! Secrets management UI: Preferences › Secrets tab (App Env secrets, Bruk credentials,
//! 1Password & Command secret managers) and the shared `secret_problem_banner`.

use factory_core::{
    CommandManagerConfig, Exposure, ONE_PASSWORD_MANAGER_ID, OnePasswordAuthMode, SecretProblem,
    SecretRecord, SecretSource, SecretState, Sharing, StoreStatus, display_with_tilde,
    has_surrounding_whitespace,
};
use nsh_base::{button, input};
use nsh_core::prelude::*;

use crate::{
    store,
    ui::{icons, palette},
};

/// A banner for a secret problem (missing secret, locked Keychain, manager auth error, etc.).
/// Red when it stopped a step or turn, amber otherwise.
pub(crate) fn secret_problem_banner(problem: SecretProblem, project_name: Option<String>) -> View {
    let blocking = problem.blocking;
    let (bg, border, fg) = if blocking {
        (
            palette::pill_error_bg(),
            palette::pill_error_border(),
            palette::pill_error_text(),
        )
    } else {
        (
            palette::pill_warning_bg(),
            palette::pill_warning_border(),
            palette::pill_warning_text(),
        )
    };
    let heading = if blocking {
        "Secret required — stopped"
    } else {
        "Secret setup"
    };
    let mut row = view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .px(px(11.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .child(icons::alert_circle(fg))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(heading)
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg),
                )
                .child(
                    text(problem.title())
                        .selectable(true)
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                ),
        );
    if let Some(label) = problem.fix.button_label() {
        let fix = problem.fix.clone();
        row = row.child(
            button()
                .test_id("secret-problem-fix")
                .accessibility_label(label)
                .on_press(move || store::apply_secret_fix(project_name.as_deref(), &fix))
                .flex_shrink_0()
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .active(|s| s.opacity(0.8))
                .child(
                    text(label)
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                ),
        );
    }
    row
}

/// Complete Preferences › Secrets tab.
pub(crate) fn preferences_secrets_tab() -> impl IntoElement {
    let add_app_open = signal(false);
    let draft_name = signal(String::new());
    let draft_value = signal(String::new());
    let draft_manager = signal("keychain".to_owned());
    let draft_exposure = signal(Exposure::McpOnly);
    let draft_all_projects = signal(true);

    let replacing_id: Signal<Option<String>> = signal(None);
    let draft_replace_value = signal(String::new());

    let add_pat_open = signal(false);
    let draft_pat_name = signal(String::new());
    let draft_pat_value = signal(String::new());

    let op_token_open = signal(false);
    let draft_op_token = signal(String::new());

    let add_mgr_open = signal(false);
    let draft_mgr_id = signal(String::new());
    let draft_mgr_name = signal(String::new());
    let draft_mgr_program = signal(String::new());
    let draft_mgr_args = signal(String::new());
    let draft_mgr_timeout = signal("15".to_owned());

    // Apply any Inbox prefill once when opening the tab.
    let (prefill_name, _prefill_scope) = store::take_secrets_prefill();
    if let Some(name) = prefill_name {
        draft_name.set(name);
        add_app_open.set(true);
    }

    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .p(px(22.0))
            .gap(px(18.0))
            .child(dynamic(move || {
                let status = store::secrets_store_status();
                let app_secrets = store::app_env_secrets();
                let pat_secrets = store::github_pat_secrets();
                let prefs = store::preferences();
                let managers = store::available_secret_managers();
                let path_change = store::db_path_change_pending();
                let legacy_files = store::legacy_db_files_found();

                let mut root = view().gap(px(16.0)).child(store_status_card(
                    &status,
                    path_change,
                    legacy_files,
                ));

                root = root
                    .child(app_env_secrets_card(
                        &app_secrets,
                        &managers,
                        add_app_open,
                        draft_name,
                        draft_value,
                        draft_manager,
                        draft_exposure,
                        draft_all_projects,
                        replacing_id,
                        draft_replace_value,
                    ))
                    .child(internal_credentials_card(
                        &pat_secrets,
                        prefs.openrouter_keys.len(),
                        add_pat_open,
                        draft_pat_name,
                        draft_pat_value,
                        replacing_id,
                        draft_replace_value,
                    ))
                    .child(secret_managers_card(
                        &prefs.secrets,
                        op_token_open,
                        draft_op_token,
                        add_mgr_open,
                        draft_mgr_id,
                        draft_mgr_name,
                        draft_mgr_program,
                        draft_mgr_args,
                        draft_mgr_timeout,
                    ))
                    .child(messages_strip());

                root
            })),
    )
}

fn store_status_card(
    status: &StoreStatus,
    path_change: Option<(std::path::PathBuf, std::path::PathBuf)>,
    legacy_files: Vec<std::path::PathBuf>,
) -> View {
    let (badge_bg, badge_fg, badge_text) = match status {
        StoreStatus::Available => (
            palette::status_success_bg(),
            palette::success_text(),
            "Keychain available".to_owned(),
        ),
        StoreStatus::Locked => (
            palette::pill_warning_bg(),
            palette::pill_warning_text(),
            "Keychain locked".to_owned(),
        ),
        StoreStatus::Unavailable(msg) => (
            palette::pill_error_bg(),
            palette::pill_error_text(),
            format!("Keychain unavailable: {msg}"),
        ),
    };

    let mut card = grouped_card("Secret Storage").child(card_divider()).child(
        view()
            .px(px(16.0))
            .py(px(12.0))
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
                                text("OS-native credential store")
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::text_primary()),
                            )
                            .child(
                                text(
                                    "Secret values are stored in your OS Keychain (never in bruk.db or repository files) and redacted from transcripts, diffs, and logs.",
                                )
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                            ),
                    )
                    .child(
                        view()
                            .px(px(8.5))
                            .py(px(3.0))
                            .rounded_full()
                            .bg(badge_bg)
                            .child(
                                text(badge_text)
                                    .test_id("prefs-secrets-store-status")
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(badge_fg),
                            ),
                    ),
            ),
    );

    if cfg!(debug_assertions) && cfg!(target_os = "macos") {
        card = card.child(card_divider()).child(
            view().px(px(16.0)).py(px(9.0)).bg(palette::card_inset_bg()).child(
                text(
                    "Dev build tip: run `task macos:setup-dev-cert` once to sign local dev builds with a stable identity so macOS Keychain does not re-prompt after each rebuild.",
                )
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
            ),
        );
    }

    if let Some((old_path, new_path)) = path_change {
        card = card.child(card_divider()).child(
            view()
                .px(px(16.0))
                .py(px(11.0))
                .gap(px(8.0))
                .bg(palette::pill_warning_bg())
                .child(
                    text(format!(
                        "Database location changed ({} → {}). Confirm whether you moved this database or copied it so Keychain entries stay isolated.",
                        display_with_tilde(&old_path),
                        display_with_tilde(&new_path)
                    ))
                    .text_size(px(12.0))
                    .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(8.0))
                        .child(secondary_button(
                            "prefs-secrets-db-moved",
                            "I Moved the Database",
                            store::confirm_database_moved,
                        ))
                        .child(secondary_button(
                            "prefs-secrets-db-cloned",
                            "This Is a Copy / Clone",
                            store::confirm_database_cloned,
                        )),
                ),
        );
    }

    if !legacy_files.is_empty() {
        let joined = legacy_files
            .iter()
            .map(|p| display_with_tilde(p))
            .collect::<Vec<_>>()
            .join(", ");
        card = card.child(card_divider()).child(
            view()
                .px(px(16.0))
                .py(px(11.0))
                .gap(px(8.0))
                .bg(palette::pill_warning_bg())
                .child(
                    text(format!(
                        "Legacy database files remain on disk ({joined}) and may still contain older plaintext secrets."
                    ))
                    .text_size(px(12.0))
                    .text_color(palette::text_primary()),
                )
                .child(
                    view().flex_row().child(secondary_button(
                        "prefs-secrets-delete-legacy",
                        "Delete Legacy Files",
                        store::delete_legacy_databases_now,
                    )),
                ),
        );
    }

    card
}

#[allow(clippy::too_many_arguments)]
fn app_env_secrets_card(
    secrets: &[SecretRecord],
    managers: &[(String, String)],
    add_open: Signal<bool>,
    draft_name: Signal<String>,
    draft_value: Signal<String>,
    draft_manager: Signal<String>,
    draft_exposure: Signal<Exposure>,
    draft_all_projects: Signal<bool>,
    replacing_id: Signal<Option<String>>,
    draft_replace_value: Signal<String>,
) -> View {
    let mut card = grouped_card_with_action(
        "App Environment Secrets",
        "Shared across projects according to their sharing scope. Defaults to MCP servers only.",
        secondary_button("prefs-secrets-add-app-btn", "Add Secret", move || {
            store::clear_secrets_messages();
            draft_name.set(String::new());
            draft_value.set(String::new());
            draft_manager.set("keychain".to_owned());
            draft_exposure.set(Exposure::McpOnly);
            draft_all_projects.set(true);
            add_open.update(|open| *open = !*open);
        }),
    );

    if secrets.is_empty() {
        card = card.child(card_divider()).child(
            view().px(px(16.0)).py(px(12.0)).child(
                text("No app-level environment secrets configured yet.")
                    .text_size(px(12.0))
                    .text_color(palette::text_muted()),
            ),
        );
    } else {
        for rec in secrets {
            card = card.child(card_divider()).child(app_secret_row(
                rec,
                replacing_id,
                draft_replace_value,
            ));
        }
    }

    if add_open.get() {
        let selected_mgr = draft_manager.get();
        let is_keychain = selected_mgr == "keychain";
        let current_val = draft_value.get();
        let ws_warning = is_keychain && has_surrounding_whitespace(&current_val);
        let exposure = draft_exposure.get();
        let all_projects = draft_all_projects.get();

        let save = move || {
            let name = draft_name.get_untracked();
            let val = draft_value.get_untracked();
            let mgr = draft_manager.get_untracked();
            let exp = draft_exposure.get_untracked();
            let sharing = if draft_all_projects.get_untracked() {
                Sharing::AllProjects
            } else {
                Sharing::NoProjects
            };
            let ok = if mgr == "keychain" {
                store::create_app_keychain_secret(&name, &val, sharing, exp)
            } else {
                store::create_app_manager_secret(&name, &mgr, &val, sharing, exp)
            };
            if ok {
                draft_name.set(String::new());
                draft_value.set(String::new());
                add_open.set(false);
            }
        };

        let mut source_picker = view()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .child(pill_toggle(
                "prefs-secrets-src-keychain",
                "Keychain",
                is_keychain,
                move || draft_manager.set("keychain".to_owned()),
            ));
        for (mgr_id, mgr_label) in managers {
            let mid = mgr_id.clone();
            let label = mgr_label.clone();
            let active = selected_mgr == mid;
            let tid = format!("prefs-secrets-src-{mid}");
            source_picker = source_picker.child(pill_toggle(&tid, &label, active, move || {
                draft_manager.set(mid.clone());
            }));
        }

        let mut value_input = input()
            .test_id("prefs-secrets-app-value-input")
            .value(move || draft_value.get())
            .on_change(move |v: &str| draft_value.set(v.to_owned()))
            .placeholder(if is_keychain {
                "Secret value (masked)"
            } else if selected_mgr == ONE_PASSWORD_MANAGER_ID {
                "op://Vault/Item/field"
            } else {
                "Secret reference"
            })
            .app_class("font-mono")
            .flex_1()
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::card_inset_bg())
            .text_size(px(12.5))
            .text_color(palette::text_primary());
        if is_keychain {
            value_input = value_input.masked(true);
        }

        let mut form = view()
            .p(px(14.0))
            .gap(px(10.0))
            .bg(palette::card_inset_bg())
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        text("Source:")
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                    )
                    .child(source_picker),
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
                                text("Environment variable name")
                                    .text_size(px(11.5))
                                    .text_color(palette::text_muted()),
                            )
                            .child(
                                input()
                                    .test_id("prefs-secrets-app-name-input")
                                    .value(move || draft_name.get())
                                    .on_change(move |v: &str| draft_name.set(v.to_owned()))
                                    .placeholder("e.g. LINEAR_API_KEY")
                                    .app_class("font-mono")
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .rounded(px(7.0))
                                    .border_1()
                                    .border_color(palette::border_color())
                                    .bg(palette::card_bg())
                                    .text_size(px(12.5))
                                    .text_color(palette::text_primary()),
                            ),
                    )
                    .child(
                        view()
                            .flex_1()
                            .gap(px(4.0))
                            .child(
                                text(if is_keychain { "Value" } else { "Reference" })
                                    .text_size(px(11.5))
                                    .text_color(palette::text_muted()),
                            )
                            .child(value_input),
                    ),
            );

        if ws_warning {
            form = form.child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(6.0))
                    .bg(palette::pill_warning_bg())
                    .child(
                        text("Value has leading or trailing whitespace.")
                            .text_size(px(11.5))
                            .text_color(palette::pill_warning_text()),
                    )
                    .child(secondary_button(
                        "prefs-secrets-trim-ws",
                        "Trim whitespace",
                        move || draft_value.update(|v| *v = v.trim().to_owned()),
                    )),
            );
        }

        if selected_mgr == ONE_PASSWORD_MANAGER_ID {
            let testing = store::secrets_test_in_progress();
            form = form.child(view().flex_row().items_center().gap(px(8.0)).child(
                secondary_button(
                    "prefs-secrets-test-op-ref",
                    if testing {
                        "Testing…"
                    } else {
                        "Test Reference"
                    },
                    move || {
                        store::test_one_password_reference_async(&draft_value.get_untracked());
                    },
                ),
            ));
        }

        form = form
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
                                text("Exposure:")
                                    .text_size(px(11.5))
                                    .text_color(palette::text_muted()),
                            )
                            .child(pill_toggle(
                                "prefs-secrets-exp-mcp",
                                "MCP servers only",
                                exposure == Exposure::McpOnly,
                                move || draft_exposure.set(Exposure::McpOnly),
                            ))
                            .child(pill_toggle(
                                "prefs-secrets-exp-agent",
                                "Agent and steps",
                                exposure == Exposure::AgentAndSteps,
                                move || draft_exposure.set(Exposure::AgentAndSteps),
                            )),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                text("Share:")
                                    .text_size(px(11.5))
                                    .text_color(palette::text_muted()),
                            )
                            .child(pill_toggle(
                                "prefs-secrets-share-all",
                                "All projects",
                                all_projects,
                                move || draft_all_projects.set(true),
                            ))
                            .child(pill_toggle(
                                "prefs-secrets-share-none",
                                "No projects",
                                !all_projects,
                                move || draft_all_projects.set(false),
                            )),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .child(secondary_button(
                        "prefs-secrets-app-cancel",
                        "Cancel",
                        move || {
                            draft_name.set(String::new());
                            draft_value.set(String::new());
                            add_open.set(false);
                        },
                    ))
                    .child(primary_button(
                        "prefs-secrets-app-save",
                        "Save Secret",
                        save,
                    )),
            );

        card = card.child(card_divider()).child(form);
    }

    card
}

fn app_secret_row(
    rec: &SecretRecord,
    replacing_id: Signal<Option<String>>,
    draft_replace_value: Signal<String>,
) -> View {
    let id = rec.id.clone();
    let id_replace = id.clone();
    let id_delete = id.clone();
    let id_exp = id.clone();
    let id_share = id.clone();
    let is_replacing = replacing_id.get().as_deref() == Some(id.as_str());
    let (sharing, exposure) = match &rec.kind {
        factory_core::SecretKind::Env {
            scope: factory_core::EnvScope::App { shared_with },
            exposure,
        } => (shared_with.clone(), *exposure),
        _ => (Sharing::AllProjects, Exposure::McpOnly),
    };
    let broad_warning =
        matches!(sharing, Sharing::AllProjects) && exposure == Exposure::AgentAndSteps;
    let value_label = match &rec.source {
        SecretSource::Keychain => {
            if rec.state == SecretState::NeedsValue {
                "Needs value".to_owned()
            } else {
                rec.preview.clone().unwrap_or_else(|| "••••••••".to_owned())
            }
        }
        SecretSource::Manager { reference, .. } => reference.clone(),
    };
    let mcp_refs = store::mcp_servers_referencing_var(&rec.name);

    let mut left = view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(3.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    text(rec.name.clone())
                        .app_class("font-mono")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(source_badge(&rec.source.badge_label()))
                .child(
                    text(value_label)
                        .app_class("font-mono")
                        .text_size(px(11.5))
                        .text_color(if rec.state == SecretState::NeedsValue {
                            palette::destructive_text()
                        } else {
                            palette::text_muted()
                        }),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    text(format!("{} · {}", sharing.label(), exposure.label()))
                        .text_size(px(11.0))
                        .text_color(palette::text_muted()),
                )
                .child(when(
                    move || broad_warning,
                    || {
                        view()
                            .px(px(6.0))
                            .py(px(1.5))
                            .rounded(px(4.0))
                            .bg(palette::pill_warning_bg())
                            .child(
                                text("Broad exposure: every project's agents & shell steps")
                                    .text_size(px(10.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(palette::pill_warning_text()),
                            )
                    },
                )),
        );

    if !mcp_refs.is_empty() {
        left = left.child(
            text(format!(
                "Referenced by MCP: {} (shell/script usage cannot be auto-detected)",
                mcp_refs.join(", ")
            ))
            .text_size(px(10.5))
            .text_color(palette::text_muted()),
        );
    }

    let next_exposure = match exposure {
        Exposure::McpOnly => Exposure::AgentAndSteps,
        Exposure::AgentAndSteps => Exposure::McpOnly,
    };
    let next_sharing = match sharing {
        Sharing::AllProjects => Sharing::NoProjects,
        _ => Sharing::AllProjects,
    };

    let mut actions = view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .child(small_action_button(
            &format!("prefs-secret-exp-{}", rec.name),
            match exposure {
                Exposure::McpOnly => "MCP Only",
                Exposure::AgentAndSteps => "Agent + Steps",
            },
            move || store::update_secret_exposure(&id_exp, next_exposure),
        ))
        .child(small_action_button(
            &format!("prefs-secret-share-{}", rec.name),
            match sharing {
                Sharing::AllProjects => "All Projects",
                _ => "No Projects",
            },
            move || store::update_app_secret_sharing(&id_share, next_sharing.clone()),
        ));

    if matches!(rec.source, SecretSource::Keychain) {
        actions = actions.child(small_action_button(
            &format!("prefs-secret-replace-{}", rec.name),
            "Replace…",
            move || {
                draft_replace_value.set(String::new());
                replacing_id.set(Some(id_replace.clone()));
            },
        ));
    }

    actions = actions.child(
        button()
            .test_id(&format!("prefs-secret-delete-{}", rec.name))
            .on_press(move || store::delete_secret_by_id(&id_delete))
            .px(px(8.0))
            .py(px(3.0))
            .rounded(px(5.0))
            .hover(|s| s.bg(palette::subtle_surface_bg()))
            .child(
                text("Delete")
                    .text_size(px(11.5))
                    .text_color(palette::destructive_text()),
            ),
    );

    let mut row = view().px(px(16.0)).py(px(10.0)).gap(px(8.0)).child(
        view()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(left)
            .child(actions),
    );

    if is_replacing {
        let save_id = id.clone();
        row = row.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    input()
                        .test_id("prefs-secret-replace-input")
                        .masked(true)
                        .flex_1()
                        .value(move || draft_replace_value.get())
                        .on_change(move |v: &str| draft_replace_value.set(v.to_owned()))
                        .placeholder("New secret value")
                        .app_class("font-mono")
                        .px(px(10.0))
                        .py(px(5.5))
                        .rounded(px(6.5))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_inset_bg())
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                )
                .child(secondary_button(
                    "prefs-secret-replace-cancel",
                    "Cancel",
                    move || {
                        draft_replace_value.set(String::new());
                        replacing_id.set(None);
                    },
                ))
                .child(primary_button(
                    "prefs-secret-replace-save",
                    "Save",
                    move || {
                        let val = draft_replace_value.get_untracked();
                        if store::replace_secret_value(&save_id, &val) {
                            draft_replace_value.set(String::new());
                            replacing_id.set(None);
                        }
                    },
                )),
        );
    }

    row
}

#[allow(clippy::too_many_arguments)]
fn internal_credentials_card(
    pats: &[SecretRecord],
    openrouter_count: usize,
    add_pat_open: Signal<bool>,
    draft_pat_name: Signal<String>,
    draft_pat_value: Signal<String>,
    replacing_id: Signal<Option<String>>,
    draft_replace_value: Signal<String>,
) -> View {
    let mut card = grouped_card_with_action(
        "Bruk Credentials (Internal)",
        "Sealed credentials used directly by Bruk (never exposed to agent processes).",
        secondary_button("prefs-secrets-add-pat-btn", "Add GitHub PAT", move || {
            store::clear_secrets_messages();
            draft_pat_name.set(String::new());
            draft_pat_value.set(String::new());
            add_pat_open.update(|open| *open = !*open);
        }),
    );

    if pats.is_empty() {
        card = card.child(card_divider()).child(
            view().px(px(16.0)).py(px(10.0)).child(
                text("No named GitHub personal access tokens saved yet.")
                    .text_size(px(12.0))
                    .text_color(palette::text_muted()),
            ),
        );
    } else {
        for pat in pats {
            let pid = pat.id.clone();
            let pid_rep = pid.clone();
            let pid_del = pid.clone();
            let projects = store::projects_using_github_pat(&pid);
            let is_replacing = replacing_id.get().as_deref() == Some(pid.as_str());
            let mut row = view().px(px(16.0)).py(px(10.0)).gap(px(8.0)).child(
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
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        text(pat.name.clone())
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(palette::text_primary()),
                                    )
                                    .child(source_badge("GitHub PAT"))
                                    .child(
                                        text(
                                            pat.preview
                                                .clone()
                                                .unwrap_or_else(|| "Needs value".to_owned()),
                                        )
                                        .app_class("font-mono")
                                        .text_size(px(11.5))
                                        .text_color(palette::text_muted()),
                                    ),
                            )
                            .child(
                                text(if projects.is_empty() {
                                    "Not selected by any project yet".to_owned()
                                } else {
                                    format!("Used by: {}", projects.join(", "))
                                })
                                .text_size(px(11.0))
                                .text_color(palette::text_muted()),
                            ),
                    )
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(small_action_button(
                                &format!("prefs-pat-replace-{}", pat.name),
                                "Replace…",
                                move || {
                                    draft_replace_value.set(String::new());
                                    replacing_id.set(Some(pid_rep.clone()));
                                },
                            ))
                            .child(
                                button()
                                    .test_id(&format!("prefs-pat-delete-{}", pat.name))
                                    .on_press(move || store::delete_secret_by_id(&pid_del))
                                    .px(px(8.0))
                                    .py(px(3.0))
                                    .rounded(px(5.0))
                                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                                    .child(
                                        text("Delete")
                                            .text_size(px(11.5))
                                            .text_color(palette::destructive_text()),
                                    ),
                            ),
                    ),
            );
            if is_replacing {
                let save_id = pid.clone();
                row = row.child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            input()
                                .test_id("prefs-pat-replace-input")
                                .masked(true)
                                .flex_1()
                                .value(move || draft_replace_value.get())
                                .on_change(move |v: &str| draft_replace_value.set(v.to_owned()))
                                .placeholder("ghp_...")
                                .app_class("font-mono")
                                .px(px(10.0))
                                .py(px(5.5))
                                .rounded(px(6.5))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_inset_bg())
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        )
                        .child(secondary_button(
                            "prefs-pat-replace-cancel",
                            "Cancel",
                            move || {
                                draft_replace_value.set(String::new());
                                replacing_id.set(None);
                            },
                        ))
                        .child(primary_button(
                            "prefs-pat-replace-save",
                            "Save",
                            move || {
                                let val = draft_replace_value.get_untracked();
                                if store::replace_secret_value(&save_id, &val) {
                                    draft_replace_value.set(String::new());
                                    replacing_id.set(None);
                                }
                            },
                        )),
                );
            }
            card = card.child(card_divider()).child(row);
        }
    }

    if add_pat_open.get() {
        card = card.child(card_divider()).child(
            view()
                .p(px(14.0))
                .gap(px(8.0))
                .bg(palette::card_inset_bg())
                .child(
                    view()
                        .flex_row()
                        .gap(px(8.0))
                        .child(
                            input()
                                .test_id("prefs-secrets-pat-name-input")
                                .flex_1()
                                .value(move || draft_pat_name.get())
                                .on_change(move |v: &str| draft_pat_name.set(v.to_owned()))
                                .placeholder("Token label (e.g. Work GitHub)")
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            input()
                                .test_id("prefs-secrets-pat-value-input")
                                .masked(true)
                                .flex_1()
                                .value(move || draft_pat_value.get())
                                .on_change(move |v: &str| draft_pat_value.set(v.to_owned()))
                                .placeholder("ghp_...")
                                .app_class("font-mono")
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded(px(7.0))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .text_size(px(12.5))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .justify_end()
                        .gap(px(8.0))
                        .child(secondary_button(
                            "prefs-secrets-pat-cancel",
                            "Cancel",
                            move || {
                                draft_pat_name.set(String::new());
                                draft_pat_value.set(String::new());
                                add_pat_open.set(false);
                            },
                        ))
                        .child(primary_button(
                            "prefs-secrets-pat-save",
                            "Save PAT",
                            move || {
                                let name = draft_pat_name.get_untracked();
                                let val = draft_pat_value.get_untracked();
                                if store::create_github_pat_secret(&name, &val).is_some() {
                                    draft_pat_name.set(String::new());
                                    draft_pat_value.set(String::new());
                                    add_pat_open.set(false);
                                }
                            },
                        )),
                ),
        );
    }

    card.child(card_divider()).child(
        view()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(16.0))
            .py(px(10.0))
            .child(
                text(format!(
                    "OpenRouter API keys ({openrouter_count} configured) are managed in Preferences › Providers."
                ))
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
            )
            .child(secondary_button(
                "prefs-secrets-open-providers",
                "Open Providers",
                || store::set_preferences_tab(factory_core::PreferencesTab::Providers),
            )),
    )
}

#[allow(clippy::too_many_arguments)]
fn secret_managers_card(
    settings: &factory_core::SecretsSettings,
    op_token_open: Signal<bool>,
    draft_op_token: Signal<String>,
    add_mgr_open: Signal<bool>,
    draft_mgr_id: Signal<String>,
    draft_mgr_name: Signal<String>,
    draft_mgr_program: Signal<String>,
    draft_mgr_args: Signal<String>,
    draft_mgr_timeout: Signal<String>,
) -> View {
    let op = &settings.one_password;
    let op_enabled = op.enabled;
    let op_mode = op.auth_mode;
    let has_sa_token = op.service_account_secret_id.is_some();
    let testing = store::secrets_test_in_progress();
    let test_result = store::secrets_test_result();

    let mut card = grouped_card_with_action(
        "Secret Managers",
        "Resolve secret references on demand from 1Password (`op`) or custom CLI tools.",
        secondary_button(
            "prefs-secrets-add-mgr-btn",
            "Add Command Manager",
            move || {
                store::clear_secrets_messages();
                draft_mgr_id.set(String::new());
                draft_mgr_name.set(String::new());
                draft_mgr_program.set(String::new());
                draft_mgr_args.set(String::new());
                draft_mgr_timeout.set("15".to_owned());
                add_mgr_open.update(|open| *open = !*open);
            },
        ),
    );

    // 1Password section
    let mut op_box = view()
        .px(px(16.0))
        .py(px(12.0))
        .gap(px(10.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    view()
                        .gap(px(2.0))
                        .child(
                            text("1Password CLI (op)")
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text("Resolves op://Vault/Item/field references via batched `op inject`.")
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(secondary_button(
                            "prefs-secrets-op-test",
                            if testing { "Testing…" } else { "Test Connection" },
                            store::test_one_password_connection_async,
                        ))
                        .child(pill_toggle(
                            "prefs-secrets-op-enable",
                            if op_enabled { "Enabled" } else { "Disabled" },
                            op_enabled,
                            move || store::set_one_password_enabled(!op_enabled),
                        )),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    text("Auth mode:")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .child(pill_toggle(
                    "prefs-secrets-op-mode-sa",
                    "Service Account (Recommended)",
                    op_mode == OnePasswordAuthMode::ServiceAccount,
                    || store::set_one_password_auth_mode(OnePasswordAuthMode::ServiceAccount),
                ))
                .child(pill_toggle(
                    "prefs-secrets-op-mode-desktop",
                    "Desktop App",
                    op_mode == OnePasswordAuthMode::DesktopApp,
                    || store::set_one_password_auth_mode(OnePasswordAuthMode::DesktopApp),
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text("op binary")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                )
                .child(
                    input()
                        .test_id("prefs-secrets-op-binary")
                        .flex_1()
                        .value({
                            let b = op.binary_path.clone();
                            move || b.clone()
                        })
                        .on_change(store::set_one_password_binary_path)
                        .placeholder("op")
                        .app_class("font-mono")
                        .px(px(9.0))
                        .py(px(5.0))
                        .rounded(px(6.5))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_inset_bg())
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                ),
        );

    if op_mode == OnePasswordAuthMode::ServiceAccount {
        op_box = op_box.child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    text(if has_sa_token {
                        "Service account token is saved in Keychain."
                    } else {
                        "No service account token saved yet."
                    })
                    .text_size(px(11.5))
                    .text_color(palette::text_muted()),
                )
                .child(secondary_button(
                    "prefs-secrets-op-token-btn",
                    if has_sa_token {
                        "Replace Token…"
                    } else {
                        "Set Token…"
                    },
                    move || {
                        draft_op_token.set(String::new());
                        op_token_open.update(|o| *o = !*o);
                    },
                )),
        );
        if op_token_open.get() {
            op_box = op_box.child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        input()
                            .test_id("prefs-secrets-op-token-input")
                            .masked(true)
                            .flex_1()
                            .value(move || draft_op_token.get())
                            .on_change(move |v: &str| draft_op_token.set(v.to_owned()))
                            .placeholder("ops_...")
                            .app_class("font-mono")
                            .px(px(10.0))
                            .py(px(5.5))
                            .rounded(px(6.5))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_inset_bg())
                            .text_size(px(12.0))
                            .text_color(palette::text_primary()),
                    )
                    .child(secondary_button(
                        "prefs-secrets-op-token-cancel",
                        "Cancel",
                        move || {
                            draft_op_token.set(String::new());
                            op_token_open.set(false);
                        },
                    ))
                    .child(primary_button(
                        "prefs-secrets-op-token-save",
                        "Save",
                        move || {
                            let val = draft_op_token.get_untracked();
                            if store::set_one_password_service_account_token(&val) {
                                draft_op_token.set(String::new());
                                op_token_open.set(false);
                            }
                        },
                    )),
            );
        }
    } else {
        let acct = op.desktop_account.clone();
        op_box = op_box
            .child(
                view()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(6.5))
                    .bg(palette::pill_warning_bg())
                    .child(
                        text("Desktop App mode requires Touch ID / interactive approval and will fail when the Mac is locked during unattended workflow runs.")
                            .text_size(px(11.5))
                            .text_color(palette::pill_warning_text()),
                    ),
            )
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .child(
                        text("Account shorthand (optional)")
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                    )
                    .child(
                        input()
                            .test_id("prefs-secrets-op-account")
                            .flex_1()
                            .value(move || acct.clone())
                            .on_change(store::set_one_password_desktop_account)
                            .placeholder("my.1password.com")
                            .app_class("font-mono")
                            .px(px(9.0))
                            .py(px(5.0))
                            .rounded(px(6.5))
                            .border_1()
                            .border_color(palette::border_color())
                            .bg(palette::card_inset_bg())
                            .text_size(px(12.0))
                            .text_color(palette::text_primary()),
                    ),
            );
    }

    if let Some(res) = test_result {
        let (ok, msg) = match res {
            Ok(m) => (true, m),
            Err(m) => (false, m),
        };
        op_box = op_box.child(
            text(msg)
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if ok {
                    palette::success_text()
                } else {
                    palette::destructive_text()
                }),
        );
    }

    card = card.child(card_divider()).child(op_box);

    // Configured Command Managers
    for mgr in &settings.command_managers {
        let mid = mgr.id.clone();
        card = card.child(card_divider()).child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(16.0))
                .py(px(10.0))
                .child(
                    view()
                        .gap(px(2.0))
                        .child(
                            text(format!("{} ({})", mgr.name, mgr.id))
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(format!("{} {}", mgr.program, mgr.args.join(" ")))
                                .app_class("font-mono")
                                .text_size(px(11.0))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    button()
                        .test_id(&format!("prefs-secrets-mgr-remove-{}", mgr.id))
                        .on_press(move || store::remove_command_manager(&mid))
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

    if add_mgr_open.get() {
        let apply_preset = move |preset: factory_core::CommandManagerPreset| {
            let cfg = preset.to_config(preset.id.to_owned());
            draft_mgr_id.set(cfg.id);
            draft_mgr_name.set(cfg.name);
            draft_mgr_program.set(cfg.program);
            draft_mgr_args.set(cfg.args.join("\n"));
            draft_mgr_timeout.set(cfg.timeout_secs.to_string());
        };
        let mut presets_row = view().flex_row().items_center().gap(px(6.0)).child(
            text("Presets:")
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        );
        for preset in factory_core::BUILT_IN_COMMAND_MANAGER_PRESETS {
            let p = *preset;
            let tid = format!("prefs-mgr-preset-{}", p.id);
            presets_row = presets_row.child(small_action_button(&tid, p.id, move || {
                apply_preset(p);
            }));
        }
        card = card.child(card_divider()).child(
            view()
                .p(px(14.0))
                .gap(px(8.0))
                .bg(palette::card_inset_bg())
                .child(presets_row)
                .child(
                    view()
                        .flex_row()
                        .gap(px(8.0))
                        .child(
                            input()
                                .test_id("prefs-secrets-mgr-id")
                                .flex_1()
                                .value(move || draft_mgr_id.get())
                                .on_change(move |v: &str| draft_mgr_id.set(v.to_owned()))
                                .placeholder("ID (e.g. bws)")
                                .app_class("font-mono")
                                .px(px(9.0))
                                .py(px(5.5))
                                .rounded(px(6.5))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            input()
                                .test_id("prefs-secrets-mgr-name")
                                .flex_1()
                                .value(move || draft_mgr_name.get())
                                .on_change(move |v: &str| draft_mgr_name.set(v.to_owned()))
                                .placeholder("Display name")
                                .px(px(9.0))
                                .py(px(5.5))
                                .rounded(px(6.5))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .gap(px(8.0))
                        .child(
                            input()
                                .test_id("prefs-secrets-mgr-program")
                                .flex_1()
                                .value(move || draft_mgr_program.get())
                                .on_change(move |v: &str| draft_mgr_program.set(v.to_owned()))
                                .placeholder("Program (e.g. bws or /opt/homebrew/bin/vault)")
                                .app_class("font-mono")
                                .px(px(9.0))
                                .py(px(5.5))
                                .rounded(px(6.5))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            input()
                                .test_id("prefs-secrets-mgr-timeout")
                                .w(px(84.0))
                                .value(move || draft_mgr_timeout.get())
                                .on_change(move |v: &str| draft_mgr_timeout.set(v.to_owned()))
                                .placeholder("15s")
                                .app_class("font-mono")
                                .px(px(9.0))
                                .py(px(5.5))
                                .rounded(px(6.5))
                                .border_1()
                                .border_color(palette::border_color())
                                .bg(palette::card_bg())
                                .text_size(px(12.0))
                                .text_color(palette::text_primary()),
                        ),
                )
                .child(
                    input()
                        .test_id("prefs-secrets-mgr-args")
                        .multiline(true)
                        .h(px(64.0))
                        .value(move || draft_mgr_args.get())
                        .on_change(move |v: &str| draft_mgr_args.set(v.to_owned()))
                        .placeholder("One argument per line (use {ref} for the secret reference)")
                        .app_class("font-mono")
                        .px(px(9.0))
                        .py(px(5.5))
                        .rounded(px(6.5))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .text_size(px(12.0))
                        .text_color(palette::text_primary()),
                )
                .child(
                    view()
                        .flex_row()
                        .justify_end()
                        .gap(px(8.0))
                        .child(secondary_button(
                            "prefs-secrets-mgr-cancel",
                            "Cancel",
                            move || {
                                add_mgr_open.set(false);
                            },
                        ))
                        .child(primary_button(
                            "prefs-secrets-mgr-save",
                            "Save Manager",
                            move || {
                                let args: Vec<String> = draft_mgr_args
                                    .get_untracked()
                                    .lines()
                                    .map(str::trim)
                                    .filter(|l| !l.is_empty())
                                    .map(str::to_owned)
                                    .collect();
                                let timeout = draft_mgr_timeout
                                    .get_untracked()
                                    .trim()
                                    .parse::<u64>()
                                    .unwrap_or(15)
                                    .clamp(1, 120);
                                let cfg = CommandManagerConfig {
                                    id: draft_mgr_id.get_untracked().trim().to_owned(),
                                    name: draft_mgr_name.get_untracked().trim().to_owned(),
                                    enabled: true,
                                    program: draft_mgr_program.get_untracked().trim().to_owned(),
                                    args,
                                    auth_env: Default::default(),
                                    timeout_secs: timeout,
                                    not_found_pattern: None,
                                };
                                if store::upsert_command_manager(cfg) {
                                    add_mgr_open.set(false);
                                }
                            },
                        )),
                ),
        );
    }

    card
}

fn messages_strip() -> View {
    let mut strip = view().gap(px(4.0));
    if let Some(err) = store::secrets_error() {
        strip = strip.child(
            text(err)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::destructive_text()),
        );
    }
    if let Some(fb) = store::secrets_feedback() {
        strip = strip.child(
            text(fb)
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    }
    strip
}

fn grouped_card(title: &'static str) -> View {
    view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view().px(px(16.0)).py(px(10.0)).child(
                text(title)
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_secondary()),
            ),
        )
}

fn grouped_card_with_action(
    title: &'static str,
    subtitle: &'static str,
    action: impl IntoElement,
) -> View {
    view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
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
                        .gap(px(2.0))
                        .child(
                            text(title)
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_secondary()),
                        )
                        .child(
                            text(subtitle)
                                .text_size(px(11.5))
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(action),
        )
}

fn card_divider() -> impl IntoElement {
    view().h(px(1.0)).w_full().bg(palette::divider_color())
}

fn source_badge(label: &str) -> View {
    view()
        .px(px(6.0))
        .py(px(1.5))
        .rounded(px(4.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::tag_pill_bg())
        .child(
            text(label.to_owned())
                .text_size(px(10.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
}

fn pill_toggle(
    test_id: &str,
    label: &str,
    active: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .on_press(on_press)
        .px(px(8.5))
        .py(px(4.0))
        .rounded(px(6.0))
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
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(label.to_owned())
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if active {
                    palette::link_blue()
                } else {
                    palette::text_secondary()
                }),
        )
}

fn small_action_button(
    test_id: &str,
    label: &str,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .on_press(on_press)
        .px(px(8.0))
        .py(px(3.5))
        .rounded(px(5.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(label.to_owned())
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
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
        .px(px(11.0))
        .py(px(5.5))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.0))
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
        .py(px(5.5))
        .rounded(px(7.0))
        .bg(palette::nav_active_bg())
        .active(|s| s.opacity(0.8))
        .child(
            text(label)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::nav_active_fg()),
        )
}
