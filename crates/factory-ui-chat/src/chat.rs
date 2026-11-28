//! Chats: free-form conversations with a project's agents, beside its workflows.
//!
//! The project's chats are listed on the left; the open chat's transcript (Markdown replies,
//! tool calls, notices) sits over a composer on the right, with an optional panel showing how
//! the agent's context window is filled. A new chat picks its provider, agent, access, and
//! where it works (the checkout or a worktree of its own) before the first message. Return
//! sends; Shift+Return adds a line. Files (screenshots, logs, any binary) attach by dropping
//! them on the chat, pasting them into the composer, or the paper clip button.

use std::collections::BTreeSet;

use factory_chat::{
    ChatAttachment, ChatContextKind, ChatContextUsage, ChatEntry, ChatGitStatus, ChatNoticeTone,
    ChatSessionSummary, ChatToolStatus, ChatWorkspace, format_bytes,
};
use factory_storage::{format_currency, format_tokens};
use factory_types::{
    DiffFileChange, DiffLineKind, ProjectSandboxMode, ProviderKind, current_unix_seconds,
};
use nsh_base::{
    SelectItem, SelectValue, TextViewPart, TextViewParts, button, input, select, text_view,
    text_view::{MarkdownOptions, TextViewLinks},
};
use nsh_core::prelude::*;

use factory_store::{
    self as store, self, ActiveChatChanges, ActiveChatContext, ActiveChatMeta, ChatEntryRow,
    PendingChatAttachment,
};
use factory_ui_kit::{icons, palette};

const LIST_WIDTH: f32 = 252.0;
const TRANSCRIPT_MAX_WIDTH: f32 = 780.0;
const NEW_CHAT_SETUP_MAX_WIDTH: f32 = 580.0;
const NO_AGENT_VALUE: &str = "__no_agent__";
const DEFAULT_MODEL_VALUE: &str = "__default_model__";
const DEFAULT_EFFORT_VALUE: &str = "__default_effort__";
/// How close to the end the transcript must be scrolled to keep following new text.
const FOLLOW_THRESHOLD: f32 = 56.0;
const CONTEXT_PANEL_WIDTH: f32 = 300.0;
const CHANGES_PANEL_WIDTH: f32 = 380.0;
/// Items of a context group listed before "+N more".
const CONTEXT_GROUP_SHOWN: usize = 8;

/// The Chats canvas for the selected project.
pub fn chat_view() -> impl IntoElement {
    let draft = signal(String::new());
    let active_id = computed(store::active_chat_id);
    // A file drag is over the conversation.
    let dragging = signal(false);

    view()
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .flex_row()
        .bg(palette::canvas_bg())
        .child(chat_list_panel(active_id))
        .child(
            view()
                .test_id("chat-drop-zone")
                .flex_1()
                .min_w(px(0.0))
                .min_h(px(0.0))
                .on_file_drag_enter(move || {
                    dragging.set(store::selected_project_id().is_some());
                })
                .on_file_drag_leave(move || dragging.set(false))
                .on_file_drop(move |event: &FileDropEvent| {
                    dragging.set(false);
                    if store::selected_project_id().is_some() {
                        store::add_chat_attachment_uris(&event.uris);
                    }
                })
                .child(dynamic(move || match active_id.get() {
                    Some(_) => chat_conversation(draft).into_element(),
                    None => view()
                        .flex_1()
                        .min_h(px(0.0))
                        .child(new_chat_intro())
                        .child(when(
                            || store::selected_project_id().is_some(),
                            move || chat_composer(draft),
                        ))
                        .into_element(),
                }))
                .child(when(move || dragging.get(), drop_overlay)),
        )
}

/// Covers the conversation while files are dragged over it.
fn drop_overlay() -> impl IntoElement {
    view()
        .absolute()
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .left(px(0.0))
        .pointer_events(PointerEvents::None)
        .p(px(14.0))
        .child(
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .rounded(px(14.0))
                .border_2()
                .border_dashed()
                .border_color(palette::link_blue())
                .bg(palette::card_selected_bg())
                .opacity(0.94)
                .child(icons::paperclip(palette::link_blue()))
                .child(
                    text("Drop files to attach them")
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text("Images go to the agent to look at; other files are passed by path.")
                        .text_size(px(12.0))
                        .text_color(palette::text_muted()),
                ),
        )
}

// --- Chat list --------------------------------------------------------------------------

fn chat_list_panel(active_id: Memo<Option<String>>) -> impl IntoElement {
    let summaries = computed(|| {
        store::selected_project_id()
            .map(store::chat_summaries_for_project)
            .unwrap_or_default()
    });
    let archived = computed(|| {
        store::selected_project_id()
            .map(store::archived_chat_summaries_for_project)
            .unwrap_or_default()
    });

    view()
        .w(px(LIST_WIDTH))
        .flex_shrink_0()
        .min_h(px(0.0))
        .border_r_1()
        .border_color(palette::border_color())
        .bg(palette::header_bg())
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(14.0))
                .pt(px(14.0))
                .pb(px(10.0))
                .child(
                    text(move || {
                        let count = summaries.get().len();
                        if count == 0 {
                            "Chats".to_owned()
                        } else {
                            format!("Chats · {count}")
                        }
                    })
                    .text_size(px(12.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_secondary()),
                )
                .child(
                    button()
                        .test_id("chat-new-btn")
                        .accessibility_label("New chat")
                        .on_press(store::start_new_chat)
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .px(px(9.0))
                        .py(px(4.5))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(move || {
                            if active_id.get().is_none() {
                                palette::card_selected_bg()
                            } else {
                                palette::control_bg()
                            }
                        })
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(icons::plus(palette::text_secondary()))
                        .child(
                            text("New chat")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
        .child(
            scroll().flex_1().min_h(px(0.0)).child(
                view()
                    .px(px(8.0))
                    .pb(px(12.0))
                    .gap(px(2.0))
                    .child(when(
                        move || summaries.get().is_empty(),
                        || {
                            view().px(px(8.0)).pt(px(4.0)).child(
                                text("No chats yet. Ask an agent about this project, or have it make a change.")
                                    .text_size(px(11.5))
                                    .text_color(palette::text_muted()),
                            )
                        },
                    ))
                    .child(each(
                        move || summaries.get(),
                        |chat: &ChatSessionSummary| chat.id.clone(),
                        move |chat, _| chat_list_row(chat, active_id),
                    ))
                    .child(when(
                        move || !archived.get().is_empty(),
                        move || archived_chats_section(archived, active_id),
                    )),
            ),
        )
}

/// The project's archived chats, folded away under their count; unfolded while one is open.
fn archived_chats_section(
    archived: Memo<Vec<ChatSessionSummary>>,
    active_id: Memo<Option<String>>,
) -> impl IntoElement {
    let unfolded = signal(false);
    let viewing_archived = move || {
        active_id.with(|active| {
            active
                .as_deref()
                .is_some_and(|id| archived.with(|chats| chats.iter().any(|chat| chat.id == id)))
        })
    };
    let open = move || unfolded.get() || viewing_archived();

    view()
        .pt(px(10.0))
        .gap(px(2.0))
        .child(
            button()
                .test_id("chat-archived-toggle")
                .accessibility_label("Archived chats")
                .on_press(move || {
                    let now_open = !(unfolded.get_untracked() || viewing_archived());
                    unfolded.set(now_open);
                })
                .flex_row()
                .items_center()
                .gap(px(5.0))
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(7.0))
                .hover(|s| s.bg(palette::sidebar_hover_bg()))
                .child(dynamic(move || {
                    if open() {
                        icons::chevron_down(palette::text_muted()).into_element()
                    } else {
                        icons::chevron_right(palette::text_muted()).into_element()
                    }
                }))
                .child(
                    text(move || format!("Archived · {}", archived.get().len()))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_muted()),
                ),
        )
        .child(when(open, move || {
            view().gap(px(2.0)).opacity(0.8).child(each(
                move || archived.get(),
                |chat: &ChatSessionSummary| chat.id.clone(),
                move |chat, _| chat_list_row(chat, active_id),
            ))
        }))
}

fn chat_list_row(
    chat: ReadSignal<ChatSessionSummary>,
    active_id: Memo<Option<String>>,
) -> impl IntoElement {
    let id = chat.get_untracked().id;
    let row_id = id.clone();
    let is_active = move || active_id.get().as_deref() == Some(row_id.as_str());
    let is_active_bg = is_active.clone();

    button()
        .test_id(&format!("chat-row-{id}"))
        .accessibility_label(move || Some(SharedString::from(chat.get().title)))
        .on_press(move || store::select_chat(&id))
        .rounded(px(8.0))
        .hover(|s| s.bg(palette::sidebar_hover_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .px(px(10.0))
                .py(px(8.0))
                .gap(px(3.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(move || {
                    if is_active_bg() {
                        palette::card_selected_border()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .bg(move || {
                    if is_active() {
                        palette::card_selected_bg()
                    } else {
                        AdaptiveColor::TRANSPARENT
                    }
                })
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(when(
                            move || chat.get().status.is_running(),
                            || {
                                view()
                                    .w(px(6.5))
                                    .h(px(6.5))
                                    .rounded_full()
                                    .bg(palette::link_blue())
                            },
                        ))
                        .child(
                            text(move || chat.get().title)
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        )
                        .child(
                            text(move || relative_time(chat.get().updated_at))
                                .flex_shrink_0()
                                .text_size(px(10.5))
                                .tabular_nums()
                                .text_color(palette::text_muted()),
                        ),
                )
                .child(
                    text(move || {
                        let chat = chat.get();
                        if chat.status.is_running() {
                            format!("{} · working…", chat.agent_label)
                        } else if chat.preview.is_empty() {
                            chat.agent_label
                        } else {
                            format!("{} · {}", chat.agent_label, chat.preview)
                        }
                    })
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(palette::text_muted()),
                ),
        )
}

/// "now", "5m", "3h", "2d" since a unix timestamp.
fn relative_time(at: i64) -> String {
    let elapsed = (current_unix_seconds() - at).max(0);
    match elapsed {
        0..60 => "now".to_owned(),
        60..3600 => format!("{}m", elapsed / 60),
        3600..86_400 => format!("{}h", elapsed / 3600),
        _ => format!("{}d", elapsed / 86_400),
    }
}

// --- New chat ---------------------------------------------------------------------------

fn new_chat_intro() -> impl IntoElement {
    view()
        .flex_1()
        .min_h(px(0.0))
        .items_center()
        .justify_center()
        .px(px(28.0))
        .py(px(16.0))
        .gap(px(10.0))
        .child(icons::chat_bubbles(28.0, palette::text_muted()))
        .child(
            text(move || match store::selected_project() {
                Some(project) => format!("Chat with an agent in {}", project.name),
                None => "Select a project to start a chat".to_owned(),
            })
            .text_size(px(17.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_primary()),
        )
        .child(
            text("Ask about the code, plan a change, or have the agent make it. Each chat keeps its own conversation; workflows stay for repeatable runs.")
                .max_w(px(470.0))
                .text_center()
                .text_size(px(12.5))
                .text_color(palette::text_muted()),
        )
        .child(when(
            || store::selected_project_id().is_some(),
            new_chat_setup_panel,
        ))
}

fn new_chat_setup_panel() -> impl IntoElement {
    view()
        .w_full()
        .max_w(px(NEW_CHAT_SETUP_MAX_WIDTH))
        .pt(px(6.0))
        .gap(px(10.0))
        .child(provider_and_model_card())
        .child(workspace_and_access_card())
        .child(agent_card())
}

fn setup_section_heading(label: &'static str) -> impl IntoElement {
    text(label)
        .text_size(px(10.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette::text_muted())
}

fn setup_field_label(label: &'static str) -> impl IntoElement {
    text(label)
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette::text_muted())
}

fn provider_and_model_card() -> impl IntoElement {
    view()
        .test_id("chat-new-provider-group")
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .px(px(14.0))
                .py(px(12.0))
                .gap(px(8.0))
                .child(setup_section_heading("PROVIDER"))
                .child(provider_radio_group()),
        )
        .child(
            view()
                .px(px(14.0))
                .py(px(11.0))
                .border_t_1()
                .border_color(palette::divider_color())
                .bg(palette::card_inset_bg())
                .child(dynamic(|| {
                    let has_effort = !store::chat_effort_options().is_empty();
                    if has_effort {
                        view()
                            .flex_row()
                            .items_start()
                            .gap(px(10.0))
                            .child(
                                view()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .gap(px(4.0))
                                    .child(setup_field_label("Model"))
                                    .child(model_picker()),
                            )
                            .child(
                                view()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .gap(px(4.0))
                                    .child(setup_field_label("Effort"))
                                    .child(effort_picker()),
                            )
                            .into_element()
                    } else {
                        view()
                            .gap(px(5.0))
                            .child(setup_field_label("Model"))
                            .child(model_picker())
                            .child(
                                text("Effort is determined by the selected Antigravity model.")
                                    .test_id("chat-new-effort-note")
                                    .text_size(px(11.0))
                                    .text_color(palette::text_muted()),
                            )
                            .into_element()
                    }
                })),
        )
}

fn provider_radio_group() -> impl IntoElement {
    view().test_id("chat-new-provider").child(dynamic(|| {
        let providers = store::chat_provider_options();
        let single = providers.len() <= 1;
        let mut row = view().flex_row().items_stretch().gap(px(8.0));
        for provider in providers {
            row = row.child(provider_radio_option(provider, single));
        }
        row.into_element()
    }))
}

fn provider_radio_option(provider: ProviderKind, single: bool) -> impl IntoElement {
    let is_selected = move || store::chat_new_provider() == provider;
    let is_selected_bg = is_selected;
    let is_selected_border = is_selected;
    let is_selected_badge = is_selected;
    let is_selected_text = is_selected;

    let tile = button()
        .test_id(&format!("chat-new-provider-{}", provider.slug()))
        .accessibility_label(provider.label())
        .on_press(move || store::set_chat_new_provider(provider))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(11.0))
        .py(px(8.5))
        .rounded(px(8.0))
        .border_1()
        .border_color(move || {
            if is_selected_border() {
                palette::card_selected_border()
            } else {
                palette::border_color()
            }
        })
        .bg(move || {
            if is_selected_bg() {
                palette::card_selected_bg()
            } else {
                palette::control_bg()
            }
        })
        .hover(move |s| {
            if is_selected() {
                s
            } else {
                s.bg(palette::subtle_surface_bg())
            }
        })
        .active(|s| s.opacity(0.85))
        .child(dynamic(move || radio_circle(is_selected())))
        .child(
            view()
                .w(px(22.0))
                .h(px(22.0))
                .rounded(px(5.5))
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .bg(move || {
                    if is_selected_badge() {
                        palette::nav_active_bg()
                    } else {
                        palette::provider_badge_bg()
                    }
                })
                .child(
                    text(provider.badge())
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || {
                            if is_selected_badge() {
                                palette::nav_active_fg()
                            } else {
                                palette::text_secondary()
                            }
                        }),
                ),
        )
        .child(
            text(provider.label())
                .truncate()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || {
                    if is_selected_text() {
                        palette::text_primary()
                    } else {
                        palette::text_secondary()
                    }
                }),
        );

    if single {
        tile.min_w(px(164.0))
    } else {
        tile.flex_1().min_w(px(0.0))
    }
}

fn radio_circle(selected: bool) -> impl IntoElement {
    let outer = view()
        .w(px(15.0))
        .h(px(15.0))
        .rounded_full()
        .flex_shrink_0()
        .items_center()
        .justify_center();

    if selected {
        outer.bg(palette::nav_active_bg()).child(
            view()
                .w(px(5.5))
                .h(px(5.5))
                .rounded_full()
                .bg(palette::nav_active_fg()),
        )
    } else {
        outer
            .border_1()
            .border_color(palette::border_color())
            .bg(palette::card_inset_bg())
    }
}

fn workspace_and_access_card() -> impl IntoElement {
    view()
        .test_id("chat-new-workspace-group")
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .px(px(14.0))
        .py(px(11.0))
        .gap(px(8.0))
        .child(setup_section_heading("WORKSPACE & ACCESS"))
        .child(
            view()
                .flex_row()
                .items_start()
                .gap(px(10.0))
                .child(when(store::chat_worktree_available, || {
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(4.0))
                        .child(setup_field_label("Work in"))
                        .child(workspace_picker())
                }))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap(px(4.0))
                        .child(setup_field_label("Access"))
                        .child(sandbox_field_picker(
                            "chat-new-access",
                            store::chat_new_sandbox,
                            store::set_chat_new_sandbox,
                        )),
                ),
        )
        .child(when(
            || store::chat_new_workspace() == ChatWorkspace::Worktree,
            || {
                text("The agent gets a new branch from HEAD in a folder of its own, so your checkout stays as it is.")
                    .text_size(px(11.0))
                    .text_color(palette::text_muted())
            },
        ))
}

fn agent_card() -> impl IntoElement {
    view()
        .test_id("chat-new-agent-group")
        .rounded(px(12.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .px(px(14.0))
        .py(px(10.0))
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(setup_section_heading("AGENT"))
                .child(
                    text("Optional persona and instructions")
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(view().w(px(220.0)).flex_shrink_0().child(agent_picker()))
}

fn workspace_picker() -> impl IntoElement {
    styled_select()
        .test_id("chat-new-workspace")
        .accessibility_label("Work in")
        .w_full()
        .items(|| {
            [ChatWorkspace::Checkout, ChatWorkspace::Worktree]
                .into_iter()
                .map(|w| SelectItem::new(w.label(), w.label()))
                .collect::<Vec<_>>()
        })
        .value(|| SelectValue::from(store::chat_new_workspace().label().to_owned()))
        .on_change(|value: &Option<SharedString>| {
            let workspace = if value.as_deref() == Some(ChatWorkspace::Worktree.label()) {
                ChatWorkspace::Worktree
            } else {
                ChatWorkspace::Checkout
            };
            store::set_chat_new_workspace(workspace);
        })
        .trigger(field_picker_trigger(|| {
            store::chat_new_workspace().label().to_owned()
        }))
}

fn field_picker_trigger(value: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .w_full()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .h(px(30.0))
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(value)
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()))
}

fn picker_trigger(caption: &'static str, value: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(10.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .child(
            text(caption)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(
            text(value)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
        .child(icons::chevron_down(palette::text_muted()))
}

fn styled_select() -> nsh_base::SelectElement {
    select()
        .cover(false)
        .placement(Placement::Bottom)
        .align(Align::Start)
        .offset(px(4.0))
        .popup_class("rounded-lg border border-border bg-popover p-1 shadow-md")
        .item_class("px-2.5 py-1.5 rounded-md text-xs text-popover-foreground focus:bg-accent selected:font-medium")
}

fn agent_picker() -> impl IntoElement {
    styled_select()
        .test_id("chat-new-agent")
        .accessibility_label("Agent")
        .w_full()
        .items(|| {
            let mut items = vec![SelectItem::new(NO_AGENT_VALUE, "No agent")];
            for (reference, name) in store::chat_agent_options() {
                items.push(SelectItem::new(reference, name));
            }
            items
        })
        .value(|| {
            SelectValue::from(store::chat_new_agent().unwrap_or_else(|| NO_AGENT_VALUE.to_owned()))
        })
        .on_change(|value: &Option<SharedString>| match value.as_deref() {
            None | Some(NO_AGENT_VALUE) => store::set_chat_new_agent(None),
            Some(reference) => store::set_chat_new_agent(Some(reference.to_owned())),
        })
        .trigger(field_picker_trigger(|| {
            let selected = store::chat_new_agent();
            selected
                .and_then(|reference| {
                    store::chat_agent_options()
                        .into_iter()
                        .find(|(r, _)| *r == reference)
                        .map(|(_, name)| name)
                })
                .unwrap_or_else(|| "No agent".to_owned())
        }))
}

fn model_picker() -> impl IntoElement {
    styled_select()
        .test_id("chat-new-model")
        .accessibility_label("Model")
        .w_full()
        .items(|| {
            let mut items = vec![SelectItem::new(
                DEFAULT_MODEL_VALUE,
                format!("Default ({})", store::chat_default_model_label()),
            )];
            let options = store::chat_model_options();
            // A model picked before the CLI's list loaded (or hidden by the family filter)
            // stays choosable.
            if let Some(current) = store::chat_new_model()
                && !options.iter().any(|(value, _)| *value == current)
            {
                items.push(SelectItem::new(current.clone(), current));
            }
            for (value, name) in options {
                items.push(SelectItem::new(value, name));
            }
            items
        })
        .value(|| {
            SelectValue::from(
                store::chat_new_model().unwrap_or_else(|| DEFAULT_MODEL_VALUE.to_owned()),
            )
        })
        .on_change(|value: &Option<SharedString>| match value.as_deref() {
            None | Some(DEFAULT_MODEL_VALUE) => store::set_chat_new_model(None),
            Some(model) => store::set_chat_new_model(Some(model.to_owned())),
        })
        .trigger(field_picker_trigger(|| match store::chat_new_model() {
            Some(model) => store::chat_model_options()
                .into_iter()
                .find(|(value, _)| *value == model)
                .map(|(_, name)| name)
                .unwrap_or(model),
            None => format!("Default ({})", store::chat_default_model_label()),
        }))
}

fn effort_picker() -> impl IntoElement {
    styled_select()
        .test_id("chat-new-effort")
        .accessibility_label("Effort")
        .w_full()
        .items(|| {
            let mut items = vec![SelectItem::new(
                DEFAULT_EFFORT_VALUE,
                format!("Default ({})", store::chat_default_effort_label()),
            )];
            for level in store::chat_effort_options() {
                items.push(SelectItem::new(level, effort_label(level)));
            }
            items
        })
        .value(|| {
            SelectValue::from(
                store::chat_new_effort().unwrap_or_else(|| DEFAULT_EFFORT_VALUE.to_owned()),
            )
        })
        .on_change(|value: &Option<SharedString>| match value.as_deref() {
            None | Some(DEFAULT_EFFORT_VALUE) => store::set_chat_new_effort(None),
            Some(level) => store::set_chat_new_effort(Some(level.to_owned())),
        })
        .trigger(field_picker_trigger(|| {
            store::chat_new_effort()
                .map(|level| effort_label(&level))
                .unwrap_or_else(|| {
                    let default = store::chat_default_effort_label();
                    if default == "provider default" {
                        "Default".to_owned()
                    } else {
                        format!("Default ({})", effort_label(&default))
                    }
                })
        }))
}

/// "xhigh" → "Extra high", "low" → "Low".
fn effort_label(level: &str) -> String {
    match level {
        "xhigh" => "Extra high".to_owned(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        }
    }
}

fn sandbox_field_picker(
    test_id: &'static str,
    current: impl Fn() -> ProjectSandboxMode + Copy + 'static,
    set: impl Fn(ProjectSandboxMode) + Copy + 'static,
) -> impl IntoElement {
    styled_select()
        .test_id(test_id)
        .accessibility_label("Access")
        .w_full()
        .items(|| {
            ProjectSandboxMode::ALL
                .iter()
                .map(|mode| SelectItem::new(mode.label(), mode.label()))
                .collect::<Vec<_>>()
        })
        .value(move || SelectValue::from(current().label().to_owned()))
        .on_change(move |value: &Option<SharedString>| {
            if let Some(mode) = ProjectSandboxMode::ALL
                .iter()
                .find(|mode| Some(mode.label()) == value.as_deref())
            {
                set(*mode);
            }
        })
        .trigger(field_picker_trigger(move || current().label().to_owned()))
}

fn sandbox_picker(
    test_id: &'static str,
    current: impl Fn() -> ProjectSandboxMode + Copy + 'static,
    set: impl Fn(ProjectSandboxMode) + Copy + 'static,
) -> impl IntoElement {
    styled_select()
        .test_id(test_id)
        .accessibility_label("Access")
        .items(|| {
            ProjectSandboxMode::ALL
                .iter()
                .map(|mode| SelectItem::new(mode.label(), mode.label()))
                .collect::<Vec<_>>()
        })
        .value(move || SelectValue::from(current().label().to_owned()))
        .on_change(move |value: &Option<SharedString>| {
            if let Some(mode) = ProjectSandboxMode::ALL
                .iter()
                .find(|mode| Some(mode.label()) == value.as_deref())
            {
                set(*mode);
            }
        })
        .trigger(picker_trigger("Access", move || {
            current().label().to_owned()
        }))
}

// --- Conversation -----------------------------------------------------------------------

fn chat_conversation(draft: Signal<String>) -> impl IntoElement {
    let meta = computed(store::active_chat_meta);
    let rows = computed(store::active_chat_rows);
    let scroller = element_ref();
    let following = signal(true);

    // Keep the newest text in view while the user is at the end of the transcript.
    effect(move || {
        rows.with(|_| {});
        if following.get_untracked() {
            scroller.scroll_to_end(false);
        }
    });

    provide_context(
        TextViewParts::new()
            .part(TextViewPart::Root, |s| s.text_size(px(13.0)))
            .part(TextViewPart::CodeBlock, |s| s.text_size(px(12.0))),
    );

    view()
        .flex_1()
        .min_h(px(0.0))
        .child(chat_header(meta))
        .child(
            view()
                .flex_1()
                .min_h(px(0.0))
                .flex_row()
                // The composer sits under the transcript, so it stays centered on it when the
                // context panel is open.
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .child(transcript(meta, rows, scroller, following))
                        .child(when(
                            move || {
                                store::selected_project_id().is_some()
                                    && meta.get().is_some_and(|m| m.archived_at.is_none())
                            },
                            move || chat_composer(draft),
                        ))
                        .child(when(
                            move || meta.get().is_some_and(|m| m.archived_at.is_some()),
                            move || archived_chat_banner(meta),
                        )),
                )
                .child(when(store::chat_context_open, context_panel))
                .child(when(store::chat_changes_open, changes_panel)),
        )
}

fn transcript(
    meta: Memo<Option<ActiveChatMeta>>,
    rows: Memo<Vec<ChatEntryRow>>,
    scroller: ElementRef,
    following: Signal<bool>,
) -> impl IntoElement {
    scroll()
        .element_ref(&scroller)
        .flex_1()
        .min_h(px(0.0))
        .on_scroll(move |event: &ScrollEvent| {
            let viewport = event.size.height.0;
            let content = event.content_size.height.0;
            let past = content - (event.offset.y.0 + viewport);
            following.set(content <= 0.0 || past <= FOLLOW_THRESHOLD);
        })
        .child(
            view()
                .items_center()
                .px(px(24.0))
                .pt(px(20.0))
                .pb(px(14.0))
                .child(
                    view()
                        .w_full()
                        .max_w(px(TRANSCRIPT_MAX_WIDTH))
                        .gap(px(14.0))
                        .child(each(
                            move || rows.get(),
                            |row: &ChatEntryRow| (row.chat_id.clone(), row.index),
                            |row, _| chat_entry_view(row),
                        ))
                        .child(when(
                            move || {
                                let running = meta.get().is_some_and(|m| m.status.is_running());
                                let streaming = rows.with(|rows| {
                                    matches!(
                                        rows.last().map(|r| &r.entry),
                                        Some(ChatEntry::Assistant {
                                            streaming: true,
                                            ..
                                        })
                                    )
                                });
                                running && !streaming
                            },
                            move || working_indicator(meta),
                        )),
                ),
        )
}

fn chat_header(meta: Memo<Option<ActiveChatMeta>>) -> impl IntoElement {
    let confirming_delete = signal(false);

    view()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(22.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(palette::divider_color())
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(2.0))
                .child(
                    text(move || meta.get().map(|m| m.title).unwrap_or_default())
                        .truncate()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(move || meta.get().map(|m| chat_meta_line(&m)).unwrap_or_default())
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(palette::text_muted()),
                ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                // While a delete waits for confirmation, its question takes the room.
                .child(when(
                    move || {
                        !confirming_delete.get()
                            && meta.get().is_some_and(|m| m.archived_at.is_some())
                    },
                    archived_pill,
                ))
                .child(when(
                    move || {
                        !confirming_delete.get()
                            && meta.get().is_some_and(|m| m.archived_at.is_none())
                    },
                    move || {
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(dynamic(move || match meta.get().and_then(|m| m.worktree) {
                                Some(worktree) => {
                                    worktree_button(worktree.branch, worktree.path).into_element()
                                }
                                None => view().into_element(),
                            }))
                            .child(when(
                                move || meta.get().is_some_and(|m| m.is_git_repo),
                                move || changes_button(meta),
                            ))
                            .child(context_button(meta))
                            .child(sandbox_picker(
                                "chat-access",
                                move || meta.get().map(|m| m.sandbox).unwrap_or_default(),
                                store::set_active_chat_sandbox,
                            ))
                    },
                ))
                .child(dynamic(move || {
                    if confirming_delete.get() {
                        let has_worktree = meta
                            .get_untracked()
                            .is_some_and(|m| m.workspace == ChatWorkspace::Worktree && m.worktree.is_some());
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                text(if has_worktree {
                                    "Delete the chat and its worktree? Uncommitted changes there are lost."
                                } else {
                                    "Delete this chat?"
                                })
                                .text_size(px(11.5))
                                .text_color(palette::text_secondary()),
                            )
                            .child(
                                button()
                                    .test_id("chat-delete-confirm")
                                    .accessibility_label("Delete")
                                    .on_press(move || {
                                        confirming_delete.set(false);
                                        if let Some(meta) = meta.get_untracked() {
                                            store::delete_chat(&meta.id);
                                        }
                                    })
                                    .px(px(9.0))
                                    .py(px(4.5))
                                    .rounded(px(6.0))
                                    .bg(palette::pill_error_bg())
                                    .border_1()
                                    .border_color(palette::pill_error_border())
                                    .active(|s| s.opacity(0.8))
                                    .child(
                                        text("Delete")
                                            .text_size(px(11.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::pill_error_text()),
                                    ),
                            )
                            .child(
                                button()
                                    .accessibility_label("Cancel")
                                    .on_press(move || confirming_delete.set(false))
                                    .px(px(9.0))
                                    .py(px(4.5))
                                    .rounded(px(6.0))
                                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                                    .child(
                                        text("Cancel")
                                            .text_size(px(11.5))
                                            .text_color(palette::text_secondary()),
                                    ),
                            )
                            .into_element()
                    } else {
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(2.0))
                            .child(when(
                                move || meta.get().is_some_and(|m| m.archived_at.is_none()),
                                move || archive_button(meta),
                            ))
                            .child(
                                button()
                                    .test_id("chat-delete-btn")
                                    .accessibility_label("Delete chat")
                                    .on_press(move || confirming_delete.set(true))
                                    .w(px(28.0))
                                    .h(px(28.0))
                                    .rounded(px(7.0))
                                    .items_center()
                                    .justify_center()
                                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                                    .child(icons::close_x(palette::text_muted())),
                            )
                            .into_element()
                    }
                })),
        )
}

fn archived_pill() -> impl IntoElement {
    view()
        .test_id("chat-archived-pill")
        .px(px(8.0))
        .py(px(3.5))
        .rounded(px(6.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::subtle_surface_bg())
        .child(
            text("Archived")
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
}

/// Stands in for the composer of an archived chat: why it's archived, and a way back.
fn archived_chat_banner(meta: Memo<Option<ActiveChatMeta>>) -> impl IntoElement {
    view()
        .items_center()
        .px(px(24.0))
        .pt(px(6.0))
        .pb(px(14.0))
        .child(
            view()
                .test_id("chat-archived-banner")
                .w_full()
                .max_w(px(TRANSCRIPT_MAX_WIDTH))
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(14.0))
                .py(px(11.0))
                .rounded(px(12.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_inset_bg())
                .child(
                    text(move || meta.get().map(|m| archived_note(&m)).unwrap_or_default())
                        .flex_1()
                        .min_w(px(0.0))
                        .text_size(px(12.0))
                        .text_color(palette::text_secondary()),
                )
                .child(
                    button()
                        .test_id("chat-unarchive-btn")
                        .accessibility_label("Unarchive chat")
                        .on_press(move || {
                            if let Some(meta) = meta.get_untracked() {
                                store::unarchive_chat(&meta.id);
                            }
                        })
                        .flex_shrink_0()
                        .px(px(11.0))
                        .py(px(5.0))
                        .rounded(px(7.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .active(|s| s.opacity(0.8))
                        .child(
                            text("Unarchive")
                                .text_size(px(12.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        )
}

/// Archives the open chat; it stays open, read-only, until unarchived.
fn archive_button(meta: Memo<Option<ActiveChatMeta>>) -> impl IntoElement {
    button()
        .test_id("chat-archive-btn")
        .accessibility_label("Archive chat")
        .disabled(move || meta.get().is_none_or(|m| m.status.is_running()))
        .on_press(move || {
            if let Some(meta) = meta.get_untracked() {
                store::archive_chat(&meta.id);
            }
        })
        .w(px(28.0))
        .h(px(28.0))
        .rounded(px(7.0))
        .items_center()
        .justify_center()
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .disabled_style(|s| s.opacity(0.4))
        .child(icons::archive_box(palette::text_muted()))
}

fn archived_note(meta: &ActiveChatMeta) -> String {
    let when = match meta.archived_at.map(relative_time).as_deref() {
        Some("now") | None => "just now".to_owned(),
        Some(ago) => format!("{ago} ago"),
    };
    match meta.worktree.as_ref() {
        // Pruned once its branch merged.
        Some(worktree) if meta.workspace == ChatWorkspace::Worktree && !worktree.path.exists() => {
            format!(
                "Archived {when}: branch {} merged, so its worktree and branch were cleaned up. \
                 Unarchive to keep chatting; the next message starts a new worktree.",
                worktree.branch
            )
        }
        Some(worktree) if meta.workspace == ChatWorkspace::Worktree => format!(
            "Archived {when}. Its worktree on {} stays until the branch merges and is pruned. \
             Unarchive to keep chatting.",
            worktree.branch
        ),
        _ => format!("Archived {when}. Unarchive to keep chatting."),
    }
}

/// The chat's worktree branch; pressing it shows the folder in Finder.
fn worktree_button(branch: String, path: std::path::PathBuf) -> impl IntoElement {
    button()
        .test_id("chat-worktree-btn")
        .accessibility_label(format!("Show worktree {branch} in Finder"))
        .on_press(move || {
            let shown = path.clone();
            let answer = nsh_core::element::reveal_path(shown.clone());
            spawn(async move {
                if answer.await.is_err() {
                    store::set_status_message(format!(
                        "Could not show '{}' in Finder. Its first message creates it.",
                        shown.display()
                    ));
                }
            });
        })
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .h(px(28.0))
        .max_w(px(220.0))
        .px(px(9.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::control_bg())
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.85))
        .child(
            view()
                .flex_shrink_0()
                .child(icons::git_branch(palette::text_muted())),
        )
        // Shrinks inside the button's width cap so a long branch name ends in an ellipsis.
        .child(
            text(branch)
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .app_class("font-mono")
                .text_size(px(11.0))
                .text_color(palette::text_secondary()),
        )
}

/// Opens the context panel; shows how full the window is once a turn has measured it.
fn context_button(meta: Memo<Option<ActiveChatMeta>>) -> impl IntoElement {
    let open = computed(store::chat_context_open);
    let context = computed(move || meta.get().and_then(|m| m.context));

    button()
        .test_id("chat-context-btn")
        .accessibility_label(move || {
            Some(
                match context.get() {
                    Some((used, window)) if window > 0 => format!(
                        "Context window, {}, {}% full",
                        context_total_label(used, window),
                        (used as f64 / window as f64 * 100.0)
                            .round()
                            .clamp(0.0, 100.0)
                    ),
                    Some((used, _)) if used > 0 => {
                        format!("Context window, {}", context_total_label(used, 0))
                    }
                    _ => "Context window".to_owned(),
                }
                .into(),
            )
        })
        .on_press(store::toggle_chat_context)
        .flex_row()
        .items_center()
        .gap(px(7.0))
        .h(px(28.0))
        .px(px(9.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(move || {
            if open.get() {
                palette::card_selected_border()
            } else {
                palette::border_color()
            }
        })
        .bg(move || {
            if open.get() {
                palette::card_selected_bg()
            } else {
                palette::control_bg()
            }
        })
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.85))
        .child(dynamic(move || {
            let fraction = context
                .get()
                .filter(|(_, window)| *window > 0)
                .map(|(used, window)| (used as f32 / window as f32).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            view()
                .w(px(30.0))
                .h(px(5.0))
                .rounded_full()
                .overflow_hidden()
                .flex_row()
                .bg(palette::segmented_track_bg())
                .child(
                    view()
                        .h_full()
                        .w(relative(if fraction > 0.0 {
                            fraction.max(0.06)
                        } else {
                            0.0
                        }))
                        .bg(fullness_color(fraction)),
                )
                .into_element()
        }))
        .child(
            text(move || match context.get() {
                Some((used, window)) if window > 0 => {
                    format!("{} / {}", compact_tokens(used), compact_tokens(window))
                }
                Some((used, _)) if used > 0 => compact_tokens(used),
                _ => "Context".to_owned(),
            })
            .tabular_nums()
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette::text_secondary()),
        )
}

/// Blue while there is room, amber past 70%, red past 90%.
fn fullness_color(fraction: f32) -> AdaptiveColor {
    if fraction >= 0.9 {
        palette::destructive_text()
    } else if fraction >= 0.7 {
        palette::warning_text()
    } else {
        palette::link_blue()
    }
}

/// "850", "17.6K", "200K", "1M", "1.5M".
fn compact_tokens(tokens: u64) -> String {
    let (value, unit) = match tokens {
        0..1_000 => return tokens.to_string(),
        1_000..1_000_000 => (tokens as f64 / 1_000.0, "K"),
        _ => (tokens as f64 / 1_000_000.0, "M"),
    };
    let digits = if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    let digits = digits.strip_suffix(".0").unwrap_or(&digits).to_owned();
    format!("{digits}{unit}")
}

/// A slice's share of the window: "62%", "1.9%", "<0.1%".
fn window_share(tokens: u64, window: u64) -> String {
    if window == 0 {
        return String::new();
    }
    let pct = tokens as f64 / window as f64 * 100.0;
    if pct >= 10.0 {
        format!("{pct:.0}%")
    } else if pct >= 0.1 {
        format!("{pct:.1}%")
    } else if tokens > 0 {
        "<0.1%".to_owned()
    } else {
        "0%".to_owned()
    }
}

// --- Context window ---------------------------------------------------------------------

fn context_panel() -> impl IntoElement {
    let context = computed(store::active_chat_context);
    let busy = move || {
        context
            .get()
            .is_some_and(|c| c.loading || c.running || !c.has_session)
    };

    view()
        .w(px(CONTEXT_PANEL_WIDTH))
        .flex_shrink_0()
        .min_h(px(0.0))
        .border_l_1()
        .border_color(palette::divider_color())
        .bg(palette::header_bg())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .px(px(14.0))
                .pt(px(12.0))
                .pb(px(8.0))
                .child(
                    text("Context window")
                        .flex_1()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(
                    button()
                        .test_id("chat-context-refresh")
                        .accessibility_label("Measure again")
                        .disabled(busy)
                        .on_press(store::refresh_active_chat_context)
                        .w(px(24.0))
                        .h(px(24.0))
                        .rounded(px(6.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .disabled_style(|s| s.opacity(0.4))
                        .child(icons::sync_arrows(palette::text_subdued())),
                )
                .child(
                    button()
                        .test_id("chat-context-close")
                        .accessibility_label("Hide context window")
                        .on_press(store::toggle_chat_context)
                        .w(px(24.0))
                        .h(px(24.0))
                        .rounded(px(6.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::close_x(palette::text_subdued())),
                ),
        )
        .child(
            scroll().flex_1().min_h(px(0.0)).child(
                view()
                    .px(px(14.0))
                    .pb(px(16.0))
                    .gap(px(14.0))
                    .child(dynamic(move || match context.get() {
                        Some(context) => context_body(context).into_element(),
                        None => view().into_element(),
                    }))
                    .child(dynamic(move || match context.get() {
                        Some(context) => token_use_section(&context),
                        None => view().into_element(),
                    })),
            ),
        )
}

fn context_body(context: ActiveChatContext) -> AnyElement {
    let Some(usage) = context.usage.clone() else {
        let (message, retry) = if context.loading {
            ("Measuring the context window…".to_owned(), false)
        } else if let Some(error) = context.error.clone() {
            (error, context.has_session)
        } else if !context.has_session || context.running {
            (
                "Send a message to see how the agent's context window fills up.".to_owned(),
                false,
            )
        } else {
            ("Not measured yet.".to_owned(), true)
        };
        return view()
            .gap(px(8.0))
            .child(
                text(message)
                    .test_id("chat-context-status")
                    .selectable(true)
                    .text_size(px(12.0))
                    .text_color(palette::text_subdued()),
            )
            .child(when(
                move || retry,
                || {
                    button()
                        .accessibility_label("Measure now")
                        .on_press(store::refresh_active_chat_context)
                        .self_start()
                        .px(px(9.0))
                        .py(px(4.5))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(
                            text("Measure now")
                                .text_size(px(11.5))
                                .text_color(palette::text_primary()),
                        )
                },
            ))
            .into_element();
    };

    let table = context_table(&usage);

    let deferred: Vec<String> = usage
        .categories
        .iter()
        .filter(|c| c.kind == ChatContextKind::Deferred && c.tokens > 0)
        .map(|c| format!("{} · {}", c.name, compact_tokens(c.tokens)))
        .collect();

    let mut body = view()
        .gap(px(14.0))
        .child(context_summary(
            &usage,
            context.loading,
            context.running,
            context.error.clone(),
        ))
        .child(table);
    if !deferred.is_empty() {
        body = body.child(
            text(format!(
                "Not in the window until the agent asks for them: {}.",
                deferred.join(", ")
            ))
            .text_size(px(11.5))
            .text_color(palette::text_subdued()),
        );
    }
    for group in &usage.groups {
        body = body.child(context_group(group));
    }
    let mut notes = Vec::new();
    if !usage.has_breakdown {
        notes.push(format!(
            "{} reports how full the window is, not what fills it.",
            context.provider.label()
        ));
    }
    if let Some(at) = usage.autocompact_at {
        notes.push(format!(
            "The conversation is compacted automatically at {} tokens.",
            compact_tokens(at)
        ));
    }
    let ago = match relative_time(usage.captured_at).as_str() {
        "now" => "just now".to_owned(),
        ago => format!("{ago} ago"),
    };
    notes.push(if usage.live && context.running {
        "Updates live as the agent works; measured in full when the reply ends.".to_owned()
    } else if usage.live {
        format!("Last updated {ago}, while a reply ran.")
    } else {
        format!("Measured {ago} after the latest reply.")
    });
    body.child(
        text(notes.join(" "))
            .text_size(px(11.5))
            .text_color(palette::text_subdued()),
    )
    .into_element()
}

fn context_summary(
    usage: &ChatContextUsage,
    updating: bool,
    running: bool,
    error: Option<String>,
) -> impl IntoElement {
    let fraction = if usage.window_tokens == 0 {
        0.0
    } else {
        (usage.used_tokens as f32 / usage.window_tokens as f32).clamp(0.0, 1.0)
    };
    let mut detail = Vec::new();
    if usage.window_tokens > 0 {
        detail.push(format!("{}% full", usage.used_percent()));
    }
    if let Some(model) = usage.model.as_ref() {
        detail.push(model.clone());
    }
    if running && usage.live {
        detail.push("live".to_owned());
    } else if updating {
        detail.push("updating…".to_owned());
    }
    view()
        .gap(px(3.0))
        .child(
            text(context_total_label(usage.used_tokens, usage.window_tokens))
                .test_id("chat-context-total")
                .tabular_nums()
                .text_size(px(17.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(when(
            {
                let shown = !detail.is_empty();
                move || shown
            },
            {
                let detail = detail.join(" · ");
                move || {
                    text(detail.clone())
                        .text_size(px(11.5))
                        .text_color(if fraction >= 0.7 {
                            fullness_color(fraction)
                        } else {
                            palette::text_subdued()
                        })
                }
            },
        ))
        .child(match error {
            Some(message) => text(message)
                .text_size(px(11.5))
                .text_color(palette::destructive_text())
                .into_element(),
            None => view().into_element(),
        })
}

/// "4.2K of 200K tokens", or "4.2K tokens" before the window's size is known.
fn context_total_label(used: u64, window: u64) -> String {
    if window > 0 {
        format!(
            "{} of {} tokens",
            compact_tokens(used),
            compact_tokens(window)
        )
    } else {
        format!("{} tokens", compact_tokens(used))
    }
}

/// Which icon a slice of the window gets.
fn context_slice_of(category: &factory_chat::ChatContextCategory) -> icons::ContextSlice {
    use icons::ContextSlice;
    match category.kind {
        ChatContextKind::Free => return ContextSlice::Free,
        ChatContextKind::Buffer => return ContextSlice::Reserve,
        ChatContextKind::Used | ChatContextKind::Deferred => {}
    }
    let id = category.id.to_ascii_lowercase();
    let name = category.name.to_ascii_lowercase();
    let either = |needle: &str| id.contains(needle) || name.contains(needle);
    if either("mcp") {
        ContextSlice::McpTools
    } else if either("tool") {
        ContextSlice::Tools
    } else if either("prompt") || either("instruction") {
        ContextSlice::Prompt
    } else if either("agent") {
        ContextSlice::Agents
    } else if either("memory") {
        ContextSlice::Memory
    } else if either("skill") {
        ContextSlice::Skills
    } else if either("reply") {
        ContextSlice::Reply
    } else if either("message") || either("conversation") {
        ContextSlice::Messages
    } else {
        ContextSlice::Other
    }
}

/// One row of a [`usage_table`].
struct UsageRow {
    id: String,
    icon: Option<(icons::ContextSlice, AdaptiveColor)>,
    name: String,
    tokens: String,
    share: String,
    /// A total row, set apart from the rows above it.
    total: bool,
}

/// A table of token counts: a column of named, colored-icon rows, their tokens, and their
/// share of a whole. Screen readers get a table with column headers; the icons are hidden
/// from them since each row's name says what it is.
fn usage_table(
    test_id: &str,
    label: &'static str,
    first_column: &'static str,
    tokens_width: f32,
    rows: Vec<UsageRow>,
) -> impl IntoElement {
    const SHARE_COL: f32 = 44.0;
    let header_cell = |label: &'static str| {
        text(label)
            .role(AccessibilityRole::ColumnHeader)
            .text_size(px(11.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_subdued())
    };
    let header = view()
        .role(AccessibilityRole::Row)
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .py(px(5.0))
        .bg(palette::table_header_bg())
        .border_b_1()
        .border_color(palette::divider_color())
        .child(header_cell(first_column).flex_1().min_w(px(0.0)))
        .child(header_cell("Tokens").w(px(tokens_width)).text_right())
        .child(header_cell("Share").w(px(SHARE_COL)).text_right());

    let last = rows.len().saturating_sub(1);
    let mut body = view().role(AccessibilityRole::RowGroup);
    for (index, row) in rows.into_iter().enumerate() {
        let weight = if row.total {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        };
        let mut name_cell = view()
            .role(AccessibilityRole::Cell)
            .flex_1()
            .min_w(px(0.0))
            .flex_row()
            .items_center()
            .gap(px(7.0));
        if let Some((slice, color)) = row.icon {
            name_cell = name_cell.child(
                view()
                    .accessibility_elements_hidden(true)
                    .child(icons::context_slice(slice, color)),
            );
        }
        name_cell = name_cell.child(
            text(row.name)
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(12.0))
                .font_weight(if row.total {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if row.total {
                    palette::text_primary()
                } else {
                    palette::text_secondary()
                }),
        );
        let mut line = view()
            .test_id(&format!("{test_id}-row-{}", row.id))
            .role(AccessibilityRole::Row)
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .py(px(6.0))
            .child(name_cell)
            .child(
                text(row.tokens)
                    .role(AccessibilityRole::Cell)
                    .w(px(tokens_width))
                    .text_right()
                    .tabular_nums()
                    .text_size(px(12.0))
                    .font_weight(weight)
                    .text_color(palette::text_primary()),
            )
            .child(
                text(row.share)
                    .role(AccessibilityRole::Cell)
                    .w(px(SHARE_COL))
                    .text_right()
                    .tabular_nums()
                    .text_size(px(11.5))
                    .text_color(palette::text_subdued()),
            );
        if row.total {
            line = line.bg(palette::table_header_bg());
        }
        if index != last {
            line = line.border_b_1().border_color(palette::divider_color());
        }
        body = body.child(line);
    }

    view()
        .test_id(test_id)
        .role(AccessibilityRole::Table)
        .label(label)
        .rounded(px(8.0))
        .border_1()
        .border_color(palette::border_color())
        .overflow_hidden()
        .bg(palette::card_bg())
        .child(header)
        .child(body)
}

/// The window's slices as a table: what is in it, the compaction reserve, then free space;
/// each with a colored icon, its tokens, and its share of the window.
fn context_table(usage: &ChatContextUsage) -> impl IntoElement {
    let window = usage.window_tokens;
    let rows = [
        ChatContextKind::Used,
        ChatContextKind::Buffer,
        ChatContextKind::Free,
    ]
    .into_iter()
    .flat_map(|kind| usage.categories.iter().filter(move |c| c.kind == kind))
    .map(|category| {
        let slice = context_slice_of(category);
        UsageRow {
            id: category.id.clone(),
            icon: Some((slice, palette::context_slice_color(slice))),
            name: category.name.clone(),
            tokens: compact_tokens(category.tokens),
            share: if window > 0 {
                window_share(category.tokens, window)
            } else {
                "—".to_owned()
            },
            total: false,
        }
    })
    .collect();
    usage_table(
        "chat-context-table",
        "Context window breakdown",
        "Portion",
        52.0,
        rows,
    )
}

// --- Token use ---------------------------------------------------------------------------

/// "17,631".
fn grouped_tokens(tokens: u64) -> String {
    let digits = tokens.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The chat's tokens by kind (input, output, cache reads and writes), live while a reply runs.
fn token_use_section(context: &ActiveChatContext) -> AnyElement {
    let tokens = context.tokens;
    let total = tokens.total();
    if total == 0 && context.recorded_tokens == 0 {
        return view().into_element();
    }
    use icons::ContextSlice;
    let mut kinds = vec![
        ("input", "Input", ContextSlice::Input, tokens.input),
        ("output", "Output", ContextSlice::Output, tokens.output),
        (
            "cache_read",
            "Cache reads",
            ContextSlice::CacheRead,
            tokens.cache_read,
        ),
    ];
    // Only Claude reports writing to the prompt cache.
    if context.provider == ProviderKind::Claude || tokens.cache_write > 0 {
        kinds.push((
            "cache_write",
            "Cache writes",
            ContextSlice::CacheWrite,
            tokens.cache_write,
        ));
    }
    let mut rows: Vec<UsageRow> = kinds
        .into_iter()
        .map(|(id, name, slice, count)| UsageRow {
            id: id.to_owned(),
            icon: Some((slice, palette::context_slice_color(slice))),
            name: name.to_owned(),
            tokens: grouped_tokens(count),
            share: if total > 0 {
                window_share(count, total)
            } else {
                "—".to_owned()
            },
            total: false,
        })
        .collect();
    rows.push(UsageRow {
        id: "total".to_owned(),
        icon: None,
        name: "Total".to_owned(),
        tokens: grouped_tokens(total),
        share: if total > 0 {
            "100%".to_owned()
        } else {
            "—".to_owned()
        },
        total: true,
    });

    let mut notes = vec![
        "Cache reads are prompt input served from the provider's cache, billed far below fresh input."
            .to_owned(),
    ];
    if context.running {
        notes.push(
            "Counts update with each model request; the provider's totals settle them when the reply ends."
                .to_owned(),
        );
    } else if context.recorded_tokens > total {
        notes.push(format!(
            "{} tokens from earlier replies weren't recorded by kind.",
            grouped_tokens(context.recorded_tokens - total)
        ));
    }

    view()
        .gap(px(8.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    text("Tokens used in this chat")
                        .role(AccessibilityRole::Header)
                        .flex_1()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(when(
                    {
                        let running = context.running;
                        move || running
                    },
                    || {
                        text("Live")
                            .test_id("chat-tokens-live")
                            .px(px(6.0))
                            .py(px(1.5))
                            .rounded_full()
                            .bg(palette::status_running_bg())
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::link_blue())
                    },
                )),
        )
        .child(usage_table(
            "chat-tokens-table",
            "Tokens used in this chat",
            "Kind",
            66.0,
            rows,
        ))
        .child(
            text(notes.join(" "))
                .text_size(px(11.5))
                .text_color(palette::text_subdued()),
        )
        .into_element()
}

fn context_group(group: &factory_chat::ChatContextGroup) -> impl IntoElement {
    let mut list = view().gap(px(4.0));
    for item in group.items.iter().take(CONTEXT_GROUP_SHOWN) {
        list = list.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    text(item.name.clone())
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(palette::text_secondary()),
                )
                .child(
                    text(compact_tokens(item.tokens))
                        .tabular_nums()
                        .text_size(px(11.5))
                        .text_color(palette::text_subdued()),
                ),
        );
    }
    let hidden = group
        .total_items
        .max(group.items.len())
        .saturating_sub(CONTEXT_GROUP_SHOWN);
    if hidden > 0 {
        list = list.child(
            text(format!("+{hidden} more"))
                .text_size(px(11.5))
                .text_color(palette::text_subdued()),
        );
    }
    view()
        .gap(px(6.0))
        .child(
            text(format!(
                "{} · {}",
                group.title,
                group.total_items.max(group.items.len())
            ))
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_secondary()),
        )
        .child(list)
}

/// "Implementer · Claude · 3 messages · 12.4K tokens · $0.08".
fn chat_meta_line(meta: &ActiveChatMeta) -> String {
    let mut parts = Vec::new();
    if meta.agent_label != meta.provider.label() {
        parts.push(meta.agent_label.clone());
    }
    parts.push(meta.provider.label().to_owned());
    if let Some(model) = meta.model.as_ref() {
        parts.push(model.clone());
    }
    if let Some(effort) = meta.reasoning_effort.as_ref() {
        parts.push(format!("{} effort", effort_label(effort)));
    }
    parts.push(match meta.message_count {
        1 => "1 message".to_owned(),
        n => format!("{n} messages"),
    });
    if meta.total_tokens > 0 {
        parts.push(format!("{} tokens", format_tokens(meta.total_tokens)));
    }
    if meta.total_cost_cents > 0 {
        parts.push(format_currency(meta.total_cost_cents));
    }
    parts.join(" · ")
}

fn chat_entry_view(row: ReadSignal<ChatEntryRow>) -> AnyElement {
    // A position's kind of entry never changes (transcripts only grow), so the row's shape is
    // fixed and only its content follows the signal.
    let first = row.get_untracked();
    match first.entry {
        ChatEntry::User { .. } => user_message(row).into_element(),
        ChatEntry::Assistant { .. } => assistant_message(row, first.provider).into_element(),
        ChatEntry::Tool { .. } => tool_call_row(row).into_element(),
        ChatEntry::Reasoning { .. } => reasoning_row(row).into_element(),
        ChatEntry::Notice { .. } => notice_row(row).into_element(),
        ChatEntry::McpProblem { problem } => view()
            .ml(px(34.0))
            .child(factory_ui_common::mcp::mcp_problem_banner(problem, None))
            .into_element(),
        ChatEntry::SecretProblem { problem } => view()
            .ml(px(34.0))
            .child(factory_ui_common::secrets::secret_problem_banner(
                problem, None,
            ))
            .into_element(),
    }
}

fn user_message(row: ReadSignal<ChatEntryRow>) -> impl IntoElement {
    let text_of = move || match row.get().entry {
        ChatEntry::User { text, .. } => text,
        _ => String::new(),
    };
    let attachments = move || match row.get().entry {
        ChatEntry::User { attachments, .. } => attachments,
        _ => Vec::new(),
    };

    view()
        .items_end()
        .gap(px(6.0))
        .pl(px(64.0))
        .child(dynamic(move || {
            let attachments = attachments();
            if attachments.is_empty() {
                return view().into_element();
            }
            let mut list = view().flex_row().flex_wrap().justify_end().gap(px(6.0));
            for attachment in attachments {
                list = list.child(sent_attachment(attachment));
            }
            list.into_element()
        }))
        .child(when(
            move || !text_of().is_empty(),
            move || {
                // Capped at the row's width so a long message wraps instead of running off
                // the leading edge.
                view()
                    .max_w(relative(1.0))
                    .min_w(px(0.0))
                    .px(px(13.0))
                    .py(px(9.0))
                    .rounded(px(12.0))
                    .bg(palette::subtle_surface_bg())
                    .border_1()
                    .border_color(palette::divider_color())
                    .child(
                        text(text_of)
                            .selectable(true)
                            .text_size(px(13.0))
                            .text_color(palette::text_primary()),
                    )
            },
        ))
}

/// A file sent with a message: a thumbnail for images, a chip for other files. Pressing it
/// opens the chat's copy in its default app.
fn sent_attachment(attachment: ChatAttachment) -> impl IntoElement {
    let path = attachment.path.clone();
    let open = move || {
        let opened = path.clone();
        let answer = nsh_core::element::open_path(opened.clone());
        spawn(async move {
            if answer.await.is_err() {
                store::set_status_message(format!("Couldn't open '{}'.", opened.display()));
            }
        });
    };
    let label = format!("Open {}", attachment.name);
    if attachment.is_image() {
        return button()
            .test_id("chat-sent-attachment")
            .accessibility_label(label)
            .on_press(open)
            .rounded(px(10.0))
            .overflow_hidden()
            .border_1()
            .border_color(palette::divider_color())
            .active(|s| s.opacity(0.85))
            .child(
                image(attachment.path.to_string_lossy().into_owned())
                    .w(px(180.0))
                    .h(px(120.0))
                    .bg(palette::subtle_surface_bg()),
            )
            .into_element();
    }
    button()
        .test_id("chat-sent-attachment")
        .accessibility_label(label)
        .on_press(open)
        .child(file_chip(
            attachment.name,
            attachment.mime_type,
            attachment.size_bytes,
        ))
        .active(|s| s.opacity(0.85))
        .into_element()
}

/// A file's icon, name and size.
fn file_chip(name: String, mime_type: String, size_bytes: u64) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .max_w(px(260.0))
        .px(px(10.0))
        .py(px(7.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::divider_color())
        .bg(palette::card_bg())
        .child(icons::file_doc(palette::text_muted()))
        .child(
            view()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(1.0))
                .child(
                    text(name)
                        .truncate()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::text_primary()),
                )
                .child(
                    text(format!(
                        "{} · {}",
                        file_kind(&mime_type),
                        format_bytes(size_bytes)
                    ))
                    .truncate()
                    .text_size(px(10.5))
                    .text_color(palette::text_muted()),
                ),
        )
}

/// "PDF", "PNG", "Text", "File".
fn file_kind(mime_type: &str) -> String {
    match mime_type {
        "application/pdf" => "PDF".to_owned(),
        "application/octet-stream" | "" => "File".to_owned(),
        "text/plain" => "Text".to_owned(),
        other => other
            .rsplit('/')
            .next()
            .map(|sub| {
                sub.trim_start_matches("x-")
                    .split('+')
                    .next()
                    .unwrap_or(sub)
            })
            .map(str::to_uppercase)
            .unwrap_or_else(|| "File".to_owned()),
    }
}

fn assistant_message(row: ReadSignal<ChatEntryRow>, provider: ProviderKind) -> impl IntoElement {
    view()
        .flex_row()
        .items_start()
        .gap(px(10.0))
        .child(provider_badge(provider))
        .child(
            view().flex_1().min_w(px(0.0)).pt(px(2.0)).child(
                text_view()
                    .markdown(move || match row.get().entry {
                        ChatEntry::Assistant { text, .. } => Some(SharedString::from(text)),
                        _ => None,
                    })
                    .options(move || MarkdownOptions {
                        // Replies talk about shell and code: `$VAR` is not a formula, and raw
                        // HTML is shown as text.
                        math: false,
                        html: false,
                        streaming: matches!(
                            row.get().entry,
                            ChatEntry::Assistant {
                                streaming: true,
                                ..
                            }
                        ),
                        ..MarkdownOptions::default()
                    })
                    .selectable(true)
                    .links(TextViewLinks::Open),
            ),
        )
}

fn provider_badge(provider: ProviderKind) -> impl IntoElement {
    view()
        .w(px(24.0))
        .h(px(24.0))
        .flex_shrink_0()
        .rounded(px(6.0))
        .bg(palette::subtle_surface_bg())
        .border_1()
        .border_color(palette::divider_color())
        .items_center()
        .justify_center()
        .child(
            text(provider.badge())
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_secondary()),
        )
}

/// One tool call: a folded line (status, tool, what it works on) that opens to everything the
/// agent passed the tool and everything it gave back, so the step can be audited in full.
fn tool_call_row(row: ReadSignal<ChatEntryRow>) -> impl IntoElement {
    let open = signal(false);
    let first = row.get_untracked();
    let index = first.index;
    let label = match &first.entry {
        ChatEntry::Tool { verb, target, .. } => format!("{verb} {target}").trim().to_owned(),
        _ => String::new(),
    };
    let tool = move || match row.get().entry {
        ChatEntry::Tool {
            verb,
            target,
            status,
            detail,
            input,
            output,
            ..
        } => ToolView {
            verb,
            target,
            status,
            detail,
            input,
            output,
        },
        _ => ToolView::default(),
    };

    let summary = step_summary()
        .child(dynamic(move || match tool().status {
            ChatToolStatus::Running => icons::status_running(palette::link_blue()).into_element(),
            ChatToolStatus::Succeeded => icons::check(palette::success_text()).into_element(),
            ChatToolStatus::Failed => icons::close_x(palette::destructive_text()).into_element(),
        }))
        .child(
            text(move || tool().verb)
                .flex_shrink_0()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
        .child(
            text(move || tool().target)
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .app_class("font-mono")
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(when(
            move || {
                let tool = tool();
                tool.status == ChatToolStatus::Failed && !tool.detail.is_empty()
            },
            move || {
                text(move || tool().detail)
                    .max_w(px(260.0))
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(palette::destructive_text())
            },
        ));

    view()
        .pl(px(34.0))
        .min_w(px(0.0))
        .gap(px(6.0))
        .child(step_header(
            &format!("chat-tool-{index}"),
            label,
            open,
            summary,
        ))
        .child(when(
            move || open.get(),
            move || {
                view()
                    .test_id(&format!("chat-tool-{index}-details"))
                    .ml(px(22.0))
                    .min_w(px(0.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(palette::divider_color())
                    .bg(palette::card_inset_bg())
                    .overflow_hidden()
                    .child(step_section(
                        move || {
                            if is_command_tool(&tool().verb) {
                                "Command"
                            } else {
                                "Input"
                            }
                        },
                        move || {
                            let tool = tool();
                            if !tool.input.is_empty() {
                                tool.input
                            } else if !tool.target.is_empty() {
                                // Older chats kept only this one-line summary.
                                tool.target
                            } else {
                                "(none)".to_owned()
                            }
                        },
                        false,
                    ))
                    .child(step_section(
                        || "Output",
                        move || {
                            let tool = tool();
                            if !tool.output.is_empty() {
                                tool.output
                            } else if tool.status == ChatToolStatus::Running {
                                "Running…".to_owned()
                            } else if !tool.detail.is_empty() {
                                tool.detail
                            } else {
                                "(no output)".to_owned()
                            }
                        },
                        true,
                    ))
            },
        ))
}

/// What a tool call row shows.
#[derive(Clone, Default, PartialEq)]
struct ToolView {
    verb: String,
    target: String,
    status: ChatToolStatus,
    detail: String,
    input: String,
    output: String,
}

/// Tools whose input is a command line.
fn is_command_tool(verb: &str) -> bool {
    matches!(verb, "Bash" | "Shell" | "BashOutput" | "PowerShell")
}

/// The agent's reasoning: folded under "Thinking", opening to the whole text.
fn reasoning_row(row: ReadSignal<ChatEntryRow>) -> impl IntoElement {
    let open = signal(false);
    let index = row.get_untracked().index;
    let text_of = move || match row.get().entry {
        ChatEntry::Reasoning { text } => text,
        _ => String::new(),
    };
    let summary = step_summary()
        .child(icons::question_bubble(palette::text_muted()))
        .child(
            text("Thinking")
                .flex_shrink_0()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
        .child(
            text(move || first_line(&text_of()))
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        );

    view()
        .pl(px(34.0))
        .min_w(px(0.0))
        .gap(px(6.0))
        .child(step_header(
            &format!("chat-reasoning-{index}"),
            "Thinking".to_owned(),
            open,
            summary,
        ))
        .child(when(
            move || open.get(),
            move || {
                view()
                    .test_id(&format!("chat-reasoning-{index}-details"))
                    .ml(px(22.0))
                    .min_w(px(0.0))
                    .px(px(11.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(palette::divider_color())
                    .bg(palette::card_inset_bg())
                    .child(
                        text(text_of)
                            .w_full()
                            .min_w(px(0.0))
                            .selectable(true)
                            .text_size(px(12.0))
                            .text_color(palette::text_secondary()),
                    )
            },
        ))
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

/// The row a folded step's summary is laid out in.
fn step_summary() -> View {
    view()
        .flex_1()
        .min_w(px(0.0))
        .flex_row()
        .items_center()
        .gap(px(8.0))
}

/// The pressable line of a folded transcript step: a disclosure chevron, then its summary.
/// Pressing it opens or closes the step's details.
fn step_header(
    test_id: &str,
    label: String,
    open: Signal<bool>,
    summary: impl IntoElement + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .on_press(move || open.update(|open| *open = !*open))
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .min_w(px(0.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .hover(|s| s.opacity(0.8))
        .child(dynamic(move || {
            if open.get() {
                icons::chevron_down(palette::text_muted()).into_element()
            } else {
                icons::chevron_right(palette::text_muted()).into_element()
            }
        }))
        .child(summary)
}

/// A labelled block of a step's details, monospaced and selectable so it can be copied.
fn step_section(
    title: impl Fn() -> &'static str + 'static,
    body: impl Fn() -> String + 'static,
    divided: bool,
) -> impl IntoElement {
    let section = view().min_w(px(0.0)).gap(px(4.0)).px(px(11.0)).py(px(8.0));
    let section = if divided {
        section.border_t_1().border_color(palette::divider_color())
    } else {
        section
    };
    section
        .child(
            text(title)
                .text_size(px(10.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_muted()),
        )
        .child(
            text(body)
                .w_full()
                .min_w(px(0.0))
                .selectable(true)
                .app_class("font-mono")
                .text_size(px(11.5))
                .text_color(palette::text_primary()),
        )
}

fn notice_row(row: ReadSignal<ChatEntryRow>) -> impl IntoElement {
    let notice = move || match row.get().entry {
        ChatEntry::Notice { text, tone } => (text, tone),
        _ => (String::new(), ChatNoticeTone::Info),
    };
    let is_error = move || notice().1 == ChatNoticeTone::Error;

    view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .ml(px(34.0))
        .px(px(11.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(move || {
            if is_error() {
                palette::pill_error_border()
            } else {
                palette::divider_color()
            }
        })
        .bg(move || {
            if is_error() {
                palette::pill_error_bg()
            } else {
                palette::subtle_surface_bg()
            }
        })
        .child(dynamic(move || {
            if is_error() {
                icons::alert_circle(palette::pill_error_text()).into_element()
            } else {
                icons::info_circle(palette::text_muted()).into_element()
            }
        }))
        .child(
            text(move || notice().0)
                .flex_1()
                .min_w(px(0.0))
                .selectable(true)
                .text_size(px(12.0))
                .text_color(move || {
                    if is_error() {
                        palette::pill_error_text()
                    } else {
                        palette::text_secondary()
                    }
                }),
        )
}

fn working_indicator(meta: Memo<Option<ActiveChatMeta>>) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .pl(px(34.0))
        .child(icons::status_running(palette::link_blue()))
        .child(
            text(move || {
                let name = meta.get().map(|m| m.agent_label).unwrap_or_default();
                format!("{name} is working…")
            })
            .text_size(px(12.0))
            .text_color(palette::text_muted()),
        )
}

// --- Composer ---------------------------------------------------------------------------

fn chat_composer(draft: Signal<String>) -> impl IntoElement {
    let meta = computed(store::active_chat_meta);
    let running = computed(move || meta.get().is_some_and(|m| m.status.is_running()));
    let pending = computed(store::chat_pending_attachments);
    let has_pending = move || pending.with(|p| !p.is_empty());
    let submit = move || {
        if running.get_untracked() {
            return;
        }
        if store::send_chat_message(&draft.get_untracked()) {
            draft.set(String::new());
        }
    };

    view()
        .items_center()
        .px(px(24.0))
        .pt(px(6.0))
        .pb(px(14.0))
        .child(
            view()
                .w_full()
                .max_w(px(TRANSCRIPT_MAX_WIDTH))
                .gap(px(5.0))
                .child(
                    view()
                        .rounded(px(12.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::card_bg())
                        .child(when(has_pending, move || pending_attachments(pending)))
                        .child(
                    view()
                        .flex_row()
                        .items_end()
                        .gap(px(8.0))
                        .pl(px(7.0))
                        .pr(px(7.0))
                        .py(px(7.0))
                        .child(attach_button())
                        .child(
                            input()
                                .test_id("chat-composer-input")
                                .multiline(true)
                                .submit_behavior(SubmitBehavior::Submit)
                                .auto_focus(true)
                                .value(move || draft.get())
                                .on_change(move |value: &str| draft.set(value.to_owned()))
                                .on_submit(move |_: &str| submit())
                                // Pasted screenshots and files copied in Finder attach; text
                                // still goes in as typed.
                                .on_paste(|paste: &PasteEvent| {
                                    for file in &paste.files {
                                        store::add_pasted_chat_attachment(&file.name, &file.data);
                                    }
                                    if !paste.uris.is_empty() {
                                        store::add_chat_attachment_uris(&paste.uris);
                                    }
                                })
                                .placeholder(move || match meta.get() {
                                    Some(meta) => format!("Message {}…", meta.agent_label),
                                    None => {
                                        let project = store::selected_project()
                                            .map(|p| p.name)
                                            .unwrap_or_default();
                                        format!("Ask anything about {project}…")
                                    }
                                })
                                .flex_1()
                                .min_w(px(0.0))
                                .max_h(px(180.0))
                                .py(px(5.0))
                                .text_size(px(13.0))
                                .text_color(palette::text_primary()),
                        )
                        .child(dynamic(move || {
                            if running.get() {
                                button()
                                    .test_id("chat-stop-btn")
                                    .accessibility_label("Stop")
                                    .on_press(store::stop_active_chat)
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .px(px(12.0))
                                    .py(px(6.5))
                                    .rounded(px(8.0))
                                    .border_1()
                                    .border_color(palette::border_color())
                                    .bg(palette::control_bg())
                                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                                    .active(|s| s.opacity(0.85))
                                    .child(icons::stop_square(palette::text_primary()))
                                    .child(
                                        text("Stop")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::text_primary()),
                                    )
                                    .into_element()
                            } else {
                                button()
                                    .test_id("chat-send-btn")
                                    .accessibility_label("Send")
                                    .disabled(move || {
                                        draft.with(|text| text.trim().is_empty()) && !has_pending()
                                    })
                                    .on_press(submit)
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .px(px(12.0))
                                    .py(px(6.5))
                                    .rounded(px(8.0))
                                    .bg(palette::nav_active_bg())
                                    .active(|s| s.opacity(0.85))
                                    .disabled_style(|s| s.opacity(0.45))
                                    .child(icons::arrow_up(palette::nav_active_fg()))
                                    .child(
                                        text("Send")
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::nav_active_fg()),
                                    )
                                    .into_element()
                            }
                        })),
                        ),
                )
                .child(
                    text(move || {
                        let place = match meta.get() {
                            Some(meta) => match meta.worktree {
                                Some(worktree) if meta.workspace == ChatWorkspace::Worktree => {
                                    format!("Works in worktree {}", worktree.branch)
                                }
                                _ if meta.workspace == ChatWorkspace::Worktree => {
                                    "Works in a new worktree".to_owned()
                                }
                                _ => "Works in the project checkout".to_owned(),
                            },
                            None if store::chat_new_workspace() == ChatWorkspace::Worktree => {
                                "Works in a new worktree".to_owned()
                            }
                            None => "Works in the project checkout".to_owned(),
                        };
                        format!(
                            "Return to send · Shift+Return for a new line · Drop or paste files to attach · {place}"
                        )
                    })
                    .test_id("chat-composer-hint")
                    .text_size(px(10.5))
                    .text_color(palette::text_muted()),
                ),
        )
}

/// The paper clip: picks files to attach.
fn attach_button() -> impl IntoElement {
    button()
        .test_id("chat-attach-btn")
        .accessibility_label("Attach files")
        .on_press(|| {
            let answer = pick_paths(
                nsh_core::file_dialog::OpenDialog::files()
                    .multiple(true)
                    .title("Attach Files")
                    .button_label("Attach"),
            );
            spawn(async move {
                match answer.await {
                    Ok(Some(paths)) if !paths.is_empty() => store::add_chat_attachments(paths),
                    Ok(_) => {}
                    Err(err) => store::set_status_message(format!("Couldn't choose files: {err}")),
                }
            });
        })
        .w(px(30.0))
        .h(px(30.0))
        .rounded(px(8.0))
        .items_center()
        .justify_center()
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.8))
        .child(icons::paperclip(palette::text_secondary()))
}

/// The files waiting to go with the next message, each removable.
fn pending_attachments(pending: Memo<Vec<PendingChatAttachment>>) -> impl IntoElement {
    view()
        .test_id("chat-pending-attachments")
        .flex_row()
        .flex_wrap()
        .gap(px(8.0))
        .px(px(10.0))
        .pt(px(10.0))
        .child(each(
            move || pending.get(),
            |attachment: &PendingChatAttachment| attachment.path.clone(),
            |attachment, _| pending_attachment(attachment.get_untracked()),
        ))
}

fn pending_attachment(attachment: PendingChatAttachment) -> impl IntoElement {
    let path = attachment.path.clone();
    let body = if attachment.is_image() {
        view()
            .rounded(px(9.0))
            .overflow_hidden()
            .border_1()
            .border_color(palette::divider_color())
            .child(
                image(attachment.path.to_string_lossy().into_owned())
                    .w(px(64.0))
                    .h(px(64.0))
                    .bg(palette::subtle_surface_bg()),
            )
            .into_element()
    } else {
        file_chip(
            attachment.name.clone(),
            attachment.mime_type.clone(),
            attachment.size_bytes,
        )
        .into_element()
    };

    view()
        .test_id("chat-pending-attachment")
        .pt(px(5.0))
        .pr(px(5.0))
        .child(body)
        .child(
            button()
                .test_id("chat-attachment-remove")
                .accessibility_label(format!("Remove {}", attachment.name))
                .on_press(move || store::remove_chat_attachment(&path))
                .absolute()
                .top(px(0.0))
                .right(px(0.0))
                .w(px(18.0))
                .h(px(18.0))
                .rounded_full()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(icons::close_x(palette::text_secondary())),
        )
}

// --- Changes & Git Drawer ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatDiffScope {
    Uncommitted,
    Branch,
}

/// Opens the Changes drawer; shows file count and +/- stats once inspected.
fn changes_button(meta: Memo<Option<ActiveChatMeta>>) -> impl IntoElement {
    let open = computed(store::chat_changes_open);
    let summary = computed(move || meta.get().and_then(|m| m.git_summary));

    button()
        .test_id("chat-changes-btn")
        .accessibility_label(move || {
            Some(
                match summary.get() {
                    Some(s) if s.uncommitted_files > 0 => format!(
                        "Changes, {} files, +{} -{}",
                        s.uncommitted_files, s.additions, s.deletions
                    ),
                    Some(s) if s.commits_ahead > 0 => {
                        format!("Changes, {} commits ahead", s.commits_ahead)
                    }
                    _ => "Changes".to_owned(),
                }
                .into(),
            )
        })
        .on_press(store::toggle_chat_changes)
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(9.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(move || {
            if open.get() {
                palette::card_selected_border()
            } else {
                palette::border_color()
            }
        })
        .bg(move || {
            if open.get() {
                palette::card_selected_bg()
            } else {
                palette::control_bg()
            }
        })
        .hover(|s| s.bg(palette::subtle_surface_bg()))
        .active(|s| s.opacity(0.85))
        .child(icons::git_diff(palette::text_muted()))
        .child(dynamic(move || match summary.get() {
            Some(s) if s.uncommitted_files > 0 => {
                let noun = if s.uncommitted_files == 1 {
                    "file"
                } else {
                    "files"
                };
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(5.0))
                    .child(
                        text(format!("{} {noun}", s.uncommitted_files))
                            .tabular_nums()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_secondary()),
                    )
                    .child(
                        text(format!("+{}", s.additions))
                            .app_class("font-mono")
                            .tabular_nums()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::diff_add_text()),
                    )
                    .child(
                        text(format!("−{}", s.deletions))
                            .app_class("font-mono")
                            .tabular_nums()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::diff_del_text()),
                    )
                    .into_element()
            }
            Some(s) if s.commits_ahead > 0 => {
                let noun = if s.commits_ahead == 1 {
                    "commit"
                } else {
                    "commits"
                };
                text(format!("{} {noun}", s.commits_ahead))
                    .tabular_nums()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::link_blue())
                    .into_element()
            }
            _ => text("Changes")
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary())
                .into_element(),
        }))
}

fn changes_panel() -> impl IntoElement {
    let changes = computed(store::active_chat_changes);
    let scope = signal(ChatDiffScope::Uncommitted);
    let pr_form_open = signal(false);
    let pr_title = signal(String::new());
    let pr_body = signal(String::new());
    let pr_draft = signal(false);
    let collapsed_files = signal(BTreeSet::<String>::new());

    let refresh_disabled = move || changes.get().is_some_and(|c| c.loading || c.busy);

    view()
        .test_id("chat-changes-panel")
        .w(px(CHANGES_PANEL_WIDTH))
        .flex_shrink_0()
        .min_h(px(0.0))
        .border_l_1()
        .border_color(palette::divider_color())
        .bg(palette::header_bg())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .px(px(14.0))
                .pt(px(12.0))
                .pb(px(8.0))
                .child(
                    text("Changes")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::text_primary()),
                )
                .child(dynamic(move || {
                    let branch = changes
                        .get()
                        .and_then(|c| c.status.map(|s| s.branch))
                        .unwrap_or_default();
                    if branch.is_empty() {
                        return view().flex_1().into_element();
                    }
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .child(
                            view()
                                .max_w(px(190.0))
                                .px(px(6.0))
                                .py(px(2.0))
                                .rounded(px(5.0))
                                .bg(palette::tag_pill_bg())
                                .child(
                                    text(branch)
                                        .truncate()
                                        .app_class("font-mono")
                                        .text_size(px(10.5))
                                        .text_color(palette::text_subdued()),
                                ),
                        )
                        .into_element()
                }))
                .child(
                    button()
                        .test_id("chat-changes-refresh")
                        .accessibility_label("Refresh changes")
                        .disabled(refresh_disabled)
                        .on_press(store::refresh_active_chat_changes)
                        .w(px(24.0))
                        .h(px(24.0))
                        .rounded(px(6.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .disabled_style(|s| s.opacity(0.4))
                        .child(icons::sync_arrows(palette::text_subdued())),
                )
                .child(
                    button()
                        .test_id("chat-changes-close")
                        .accessibility_label("Hide changes")
                        .on_press(store::toggle_chat_changes)
                        .w(px(24.0))
                        .h(px(24.0))
                        .rounded(px(6.0))
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::close_x(palette::text_subdued())),
                ),
        )
        .child(
            scroll().flex_1().min_h(px(0.0)).child(
                view()
                    .px(px(14.0))
                    .pb(px(16.0))
                    .gap(px(12.0))
                    .child(dynamic(move || match changes.get() {
                        Some(data) => changes_panel_body(
                            data,
                            scope,
                            pr_form_open,
                            pr_title,
                            pr_body,
                            pr_draft,
                            collapsed_files,
                        )
                        .into_element(),
                        None => view().into_element(),
                    })),
            ),
        )
}

#[allow(clippy::too_many_arguments)]
fn changes_panel_body(
    data: ActiveChatChanges,
    scope: Signal<ChatDiffScope>,
    pr_form_open: Signal<bool>,
    pr_title: Signal<String>,
    pr_body: Signal<String>,
    pr_draft: Signal<bool>,
    collapsed_files: Signal<BTreeSet<String>>,
) -> impl IntoElement {
    let mut body = view().gap(px(10.0));

    // Action toolbar
    body = body.child(changes_action_bar(
        &data,
        pr_form_open,
        pr_title,
        pr_body,
        pr_draft,
    ));

    // Inline Open PR form
    if pr_form_open.get() {
        body = body.child(pr_form_card(
            &data.default_branch,
            data.busy || data.running,
            pr_form_open,
            pr_title,
            pr_body,
            pr_draft,
        ));
    }

    // PR URL banner
    if let Some(url) = data.pr_url.clone() {
        body = body.child(
            view()
                .test_id("chat-pr-link-card")
                .p(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::intake_banner_border())
                .bg(palette::intake_banner_bg())
                .gap(px(6.0))
                .child(
                    text("Pull request ready")
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette::link_blue()),
                )
                .child(
                    text(url)
                        .selectable(true)
                        .app_class("font-mono")
                        .text_size(px(11.0))
                        .text_color(palette::text_secondary()),
                )
                .child(
                    button()
                        .test_id("chat-pr-open-link")
                        .accessibility_label("Open pull request in browser")
                        .on_press(store::open_active_chat_pr_url)
                        .self_start()
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(palette::border_color())
                        .bg(palette::control_bg())
                        .hover(|s| s.bg(palette::subtle_surface_bg()))
                        .child(icons::external_link(palette::link_blue()))
                        .child(
                            text("Open in Browser")
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::text_primary()),
                        ),
                ),
        );
    }

    // Feedback & error callouts
    if let Some(feedback) = data.feedback.clone() {
        body = body.child(
            view()
                .test_id("chat-changes-feedback")
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::status_success_bg())
                .child(
                    text(feedback)
                        .selectable(true)
                        .text_size(px(11.5))
                        .text_color(palette::success_text()),
                ),
        );
    }

    if let Some(error) = data.error.clone() {
        body = body.child(
            view()
                .test_id("chat-changes-error")
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette::pill_error_border())
                .bg(palette::pill_error_bg())
                .child(
                    text(error)
                        .selectable(true)
                        .text_size(px(11.5))
                        .text_color(palette::pill_error_text()),
                ),
        );
    }

    let Some(status) = data.status else {
        let msg = if data.loading {
            "Checking git status…".to_owned()
        } else if data.is_worktree && !data.working_dir.is_dir() {
            "The worktree is created when you send the first message.".to_owned()
        } else {
            "No git status loaded yet.".to_owned()
        };
        return body.child(
            text(msg)
                .test_id("chat-changes-empty")
                .text_size(px(12.0))
                .text_color(palette::text_subdued()),
        );
    };

    // Commits ahead section
    if status.commits_ahead > 0 {
        body = body.child(commits_ahead_card(&status));
    }

    // Scope switcher when both uncommitted and committed branch diffs exist
    let effective_scope = if status.has_uncommitted_changes() && status.commits_ahead > 0 {
        body = body.child(diff_scope_switcher(&status, scope));
        scope.get()
    } else if status.has_uncommitted_changes() {
        ChatDiffScope::Uncommitted
    } else if status.commits_ahead > 0 {
        ChatDiffScope::Branch
    } else {
        ChatDiffScope::Uncommitted
    };

    let (diffs, add_total, del_total, section_label) = match effective_scope {
        ChatDiffScope::Uncommitted => (
            &status.uncommitted_diffs,
            status.uncommitted_additions(),
            status.uncommitted_deletions(),
            "Uncommitted changes",
        ),
        ChatDiffScope::Branch => (
            &status.committed_diffs,
            status.committed_additions(),
            status.committed_deletions(),
            "Branch diff",
        ),
    };

    if diffs.is_empty() {
        return body.child(
            view()
                .test_id("chat-changes-empty")
                .p(px(16.0))
                .rounded(px(9.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::card_bg())
                .items_center()
                .child(
                    text("Working tree is clean.")
                        .text_size(px(12.0))
                        .text_color(palette::text_subdued()),
                ),
        );
    }

    let collapsed = collapsed_files.get();
    let all_collapsed = diffs.iter().all(|f| collapsed.contains(&f.path));
    let all_paths: Vec<String> = diffs.iter().map(|f| f.path.clone()).collect();
    let toggle_all_label = if all_collapsed {
        "Expand all"
    } else {
        "Collapse all"
    };

    let file_noun = if diffs.len() == 1 { "file" } else { "files" };
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .pt(px(2.0))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        text(format!("{section_label} · {} {file_noun}", diffs.len()))
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_subdued()),
                    )
                    .child(
                        text(format!("+{add_total}"))
                            .app_class("font-mono")
                            .tabular_nums()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::diff_add_text()),
                    )
                    .child(
                        text(format!("−{del_total}"))
                            .app_class("font-mono")
                            .tabular_nums()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::diff_del_text()),
                    ),
            )
            .child(
                button()
                    .test_id("chat-diff-toggle-all")
                    .accessibility_label(toggle_all_label)
                    .on_press(move || {
                        collapsed_files.update(|set| {
                            if all_collapsed {
                                for path in &all_paths {
                                    set.remove(path);
                                }
                            } else {
                                for path in &all_paths {
                                    set.insert(path.clone());
                                }
                            }
                        });
                    })
                    .px(px(6.5))
                    .py(px(2.5))
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(palette::border_color())
                    .bg(palette::control_bg())
                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                    .child(
                        text(toggle_all_label)
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette::text_secondary()),
                    ),
            ),
    );

    for file in diffs {
        let is_untracked = effective_scope == ChatDiffScope::Uncommitted
            && status.untracked_paths.contains(&file.path);
        let is_collapsed = collapsed.contains(&file.path);
        body = body.child(chat_diff_file_card(
            file,
            is_untracked,
            is_collapsed,
            collapsed_files,
        ));
    }

    body
}

fn changes_action_bar(
    data: &ActiveChatChanges,
    pr_form_open: Signal<bool>,
    pr_title: Signal<String>,
    pr_body: Signal<String>,
    pr_draft: Signal<bool>,
) -> impl IntoElement {
    let status = data.status.as_ref();
    let has_uncommitted = status.is_some_and(ChatGitStatus::has_uncommitted_changes);
    let can_open_pr = status.is_some_and(ChatGitStatus::can_open_pr);
    let has_any = status.is_some_and(ChatGitStatus::has_any_changes);
    let locked = data.busy || data.running;
    let default_pr_title = data.title.clone();
    let default_draft = data.open_prs_as_draft;
    let working_dir = data.working_dir.clone();

    let mut row = view().flex_row().flex_wrap().items_center().gap(px(6.0));

    // Commit button — asks the chat's agent to review and commit the uncommitted changes.
    row = row.child(action_chip_button(
        "chat-changes-commit-btn",
        "Commit",
        false,
        !has_uncommitted || locked,
        icons::git_commit(palette::text_secondary()),
        move || {
            pr_form_open.set(false);
            store::ask_agent_to_commit_active_chat();
        },
    ));

    // Open PR button (worktree or feature branch)
    let show_pr = data.is_worktree || can_open_pr;
    if show_pr {
        row = row.child(action_chip_button(
            "chat-changes-pr-toggle",
            "Open PR",
            pr_form_open.get(),
            !can_open_pr || locked,
            icons::git_pr(palette::text_secondary()),
            move || {
                let next = !pr_form_open.get_untracked();
                pr_form_open.set(next);
                if next {
                    if pr_title.get_untracked().trim().is_empty() {
                        pr_title.set(default_pr_title.clone());
                    }
                    pr_body.set(String::new());
                    pr_draft.set(default_draft);
                }
            },
        ));
    }

    // Apply to Checkout (worktree chats only)
    if data.is_worktree {
        row = row.child(action_chip_button(
            "chat-changes-apply-btn",
            "Apply to Checkout",
            false,
            !has_any || locked,
            icons::download_tray(palette::text_secondary()),
            store::apply_active_chat_to_checkout,
        ));
    }

    // Open in Terminal
    row = row.child(action_chip_button(
        "chat-changes-terminal-btn",
        "Terminal",
        false,
        !working_dir.is_dir(),
        icons::terminal_prompt(palette::text_secondary()),
        store::open_active_chat_in_terminal,
    ));

    // Reveal in Finder
    let finder_dir = working_dir.clone();
    row = row.child(action_chip_button(
        "chat-changes-finder-btn",
        "Finder",
        false,
        !working_dir.is_dir(),
        icons::folder(palette::text_secondary()),
        move || {
            let shown = finder_dir.clone();
            let answer = nsh_core::element::reveal_path(shown.clone());
            spawn(async move {
                if answer.await.is_err() {
                    store::set_status_message(format!(
                        "Could not show '{}' in Finder.",
                        shown.display()
                    ));
                }
            });
        },
    ));

    row
}

fn action_chip_button(
    test_id: &'static str,
    label: &'static str,
    active: bool,
    disabled: bool,
    icon: impl IntoElement,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label)
        .disabled(disabled)
        .on_press(on_press)
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .h(px(27.0))
        .px(px(8.5))
        .rounded(px(6.5))
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
        .disabled_style(|s| s.opacity(0.42))
        .child(icon)
        .child(
            text(label)
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
}

fn pr_form_card(
    base_branch: &str,
    disabled: bool,
    pr_form_open: Signal<bool>,
    pr_title: Signal<String>,
    pr_body: Signal<String>,
    pr_draft: Signal<bool>,
) -> impl IntoElement {
    let submit = move || {
        let title = pr_title.get_untracked();
        if title.trim().is_empty() {
            return;
        }
        let body = pr_body.get_untracked();
        let draft = pr_draft.get_untracked();
        pr_form_open.set(false);
        store::open_active_chat_pr(title, body, draft);
    };

    view()
        .test_id("chat-pr-form")
        .p(px(10.0))
        .rounded(px(8.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .gap(px(8.0))
        .child(
            text(format!("Push branch & open PR into {base_branch}"))
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            input()
                .test_id("chat-pr-title-input")
                .value(move || pr_title.get())
                .placeholder("Pull request title")
                .on_change(move |v: &str| pr_title.set(v.to_owned()))
                .on_submit(move |_: &str| submit())
                .h(px(30.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .text_size(px(12.0))
                .text_color(palette::text_primary()),
        )
        .child(
            input()
                .test_id("chat-pr-body-input")
                .value(move || pr_body.get())
                .placeholder("Description (optional)")
                .on_change(move |v: &str| pr_body.set(v.to_owned()))
                .h(px(30.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette::border_color())
                .bg(palette::control_bg())
                .text_size(px(12.0))
                .text_color(palette::text_primary()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    button()
                        .test_id("chat-pr-draft-toggle")
                        .accessibility_label("Open as draft pull request")
                        .on_press(move || pr_draft.set(!pr_draft.get_untracked()))
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .px(px(7.0))
                        .py(px(3.5))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(move || {
                            if pr_draft.get() {
                                palette::card_selected_border()
                            } else {
                                palette::border_color()
                            }
                        })
                        .bg(move || {
                            if pr_draft.get() {
                                palette::card_selected_bg()
                            } else {
                                palette::control_bg()
                            }
                        })
                        .child(
                            text(move || {
                                if pr_draft.get() {
                                    "✓ Draft PR"
                                } else {
                                    "Draft PR"
                                }
                            })
                            .text_size(px(11.0))
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
                                .accessibility_label("Cancel pull request")
                                .on_press(move || pr_form_open.set(false))
                                .px(px(9.0))
                                .py(px(4.5))
                                .rounded(px(6.0))
                                .hover(|s| s.bg(palette::subtle_surface_bg()))
                                .child(
                                    text("Cancel")
                                        .text_size(px(11.5))
                                        .text_color(palette::text_secondary()),
                                ),
                        )
                        .child(
                            button()
                                .test_id("chat-pr-submit")
                                .accessibility_label("Push and open pull request")
                                .disabled(move || disabled || pr_title.get().trim().is_empty())
                                .on_press(submit)
                                .px(px(10.0))
                                .py(px(4.5))
                                .rounded(px(6.0))
                                .bg(palette::nav_active_bg())
                                .disabled_style(|s| s.opacity(0.45))
                                .child(
                                    text("Push & Open PR")
                                        .text_size(px(11.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(palette::nav_active_fg()),
                                ),
                        ),
                ),
        )
}

fn commits_ahead_card(status: &ChatGitStatus) -> impl IntoElement {
    let noun = if status.commits_ahead == 1 {
        "commit"
    } else {
        "commits"
    };
    let mut card = view()
        .test_id("chat-changes-commits")
        .rounded(px(8.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            view()
                .px(px(10.0))
                .py(px(6.5))
                .bg(palette::card_inset_bg())
                .border_b_1()
                .border_color(palette::divider_color())
                .child(
                    text(format!(
                        "{} {noun} ahead of {}",
                        status.commits_ahead, status.base_branch
                    ))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::text_secondary()),
                ),
        );

    let mut list = view().px(px(10.0)).py(px(6.0)).gap(px(4.0));
    for commit in status.commits.iter().take(6) {
        list = list.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(7.0))
                .child(
                    text(commit.short_sha.clone())
                        .app_class("font-mono")
                        .text_size(px(10.5))
                        .text_color(palette::link_blue()),
                )
                .child(
                    text(commit.subject.clone())
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(11.5))
                        .text_color(palette::text_primary()),
                ),
        );
    }
    if status.commits_ahead > 6 {
        list = list.child(
            text(format!("+{} more", status.commits_ahead - 6))
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
        );
    }
    card = card.child(list);
    card
}

fn diff_scope_switcher(status: &ChatGitStatus, scope: Signal<ChatDiffScope>) -> impl IntoElement {
    let active = scope.get();
    view()
        .flex_row()
        .p(px(2.5))
        .rounded(px(7.0))
        .bg(palette::segmented_track_bg())
        .gap(px(2.0))
        .child(scope_segment_button(
            "chat-diff-scope-uncommitted",
            format!("Uncommitted ({})", status.uncommitted_file_count()),
            active == ChatDiffScope::Uncommitted,
            move || scope.set(ChatDiffScope::Uncommitted),
        ))
        .child(scope_segment_button(
            "chat-diff-scope-branch",
            format!("Branch ({})", status.committed_file_count()),
            active == ChatDiffScope::Branch,
            move || scope.set(ChatDiffScope::Branch),
        ))
}

fn scope_segment_button(
    test_id: &'static str,
    label: String,
    selected: bool,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .test_id(test_id)
        .accessibility_label(label.clone())
        .on_press(on_press)
        .flex_1()
        .h(px(24.0))
        .rounded(px(5.5))
        .items_center()
        .justify_center()
        .bg(if selected {
            palette::control_bg()
        } else {
            AdaptiveColor::TRANSPARENT
        })
        .child(
            text(label)
                .text_size(px(11.0))
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    palette::text_primary()
                } else {
                    palette::text_subdued()
                }),
        )
}

fn chat_diff_file_card(
    file: &DiffFileChange,
    is_untracked: bool,
    collapsed: bool,
    collapsed_files: Signal<BTreeSet<String>>,
) -> impl IntoElement {
    let toggle_path = file.path.clone();
    let mut card = view()
        .test_id("chat-diff-file-card")
        .rounded(px(8.5))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
        .child(
            button()
                .accessibility_label(format!("Toggle diff for {}", file.path))
                .on_press(move || {
                    collapsed_files.update(|set| {
                        if !set.remove(&toggle_path) {
                            set.insert(toggle_path.clone());
                        }
                    });
                })
                .w_full()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(7.5))
                .bg(palette::card_inset_bg())
                .hover(|s| s.bg(palette::subtle_surface_bg()))
                .child(
                    view()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(if collapsed {
                            icons::chevron_right(palette::text_muted()).into_element()
                        } else {
                            icons::chevron_down(palette::text_muted()).into_element()
                        })
                        .child(
                            text(file.path.clone())
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .app_class("font-mono")
                                .text_size(px(11.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette::text_primary()),
                        )
                        .child(when(
                            move || is_untracked,
                            || {
                                view()
                                    .px(px(5.0))
                                    .py(px(1.0))
                                    .rounded(px(4.0))
                                    .bg(palette::status_success_bg())
                                    .child(
                                        text("New")
                                            .text_size(px(9.5))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(palette::success_text()),
                                    )
                            },
                        )),
                )
                .child(
                    view()
                        .flex_shrink_0()
                        .flex_row()
                        .items_center()
                        .gap(px(5.0))
                        .child(
                            text(format!("+{}", file.additions))
                                .app_class("font-mono")
                                .tabular_nums()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::diff_add_text()),
                        )
                        .child(
                            text(format!("−{}", file.deletions))
                                .app_class("font-mono")
                                .tabular_nums()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::diff_del_text()),
                        ),
                ),
        );

    if !collapsed {
        let mut lines_col = view()
            .border_t_1()
            .border_color(palette::divider_color())
            .py(px(5.0));
        if !file.hunk_header.is_empty() {
            lines_col = lines_col.child(
                text(file.hunk_header.clone())
                    .px(px(10.0))
                    .py(px(2.5))
                    .app_class("font-mono")
                    .text_size(px(10.5))
                    .text_color(palette::text_muted()),
            );
        }
        for line in &file.lines {
            let (prefix, bg, fg) = match line.kind {
                DiffLineKind::Context => {
                    (" ", AdaptiveColor::TRANSPARENT, palette::text_secondary())
                }
                DiffLineKind::Added => ("+", palette::diff_add_bg(), palette::diff_add_text()),
                DiffLineKind::Removed => ("-", palette::diff_del_bg(), palette::diff_del_text()),
            };
            lines_col = lines_col.child(
                view().px(px(10.0)).py(px(1.5)).bg(bg).child(
                    text(format!("{prefix}{}", line.text))
                        .selectable(true)
                        .app_class("font-mono")
                        .text_size(px(11.0))
                        .text_color(fg),
                ),
            );
        }
        card = card.child(lines_col);
    }

    card
}
