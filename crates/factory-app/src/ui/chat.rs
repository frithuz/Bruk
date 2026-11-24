//! Chats: free-form conversations with a project's agents, beside its workflows.
//!
//! The project's chats are listed on the left; the open chat's transcript (Markdown replies,
//! tool calls, notices) sits over a composer on the right, with an optional panel showing how
//! the agent's context window is filled. A new chat picks its provider, agent, access, and
//! where it works (the checkout or a worktree of its own) before the first message. Return
//! sends; Shift+Return adds a line. Files (screenshots, logs, any binary) attach by dropping
//! them on the chat, pasting them into the composer, or the paper clip button.

use factory_core::{
    ChatAttachment, ChatContextKind, ChatContextUsage, ChatEntry, ChatNoticeTone,
    ChatSessionSummary, ChatToolStatus, ChatWorkspace, ProjectColor, ProjectSandboxMode,
    ProviderKind, current_unix_seconds, format_bytes, format_currency, format_tokens,
};
use nsh_base::{
    SelectItem, SelectValue, TextViewPart, TextViewParts, button, input, select, text_view,
    text_view::{MarkdownOptions, TextViewLinks},
};
use nsh_core::prelude::*;

use crate::{
    store::{self, ActiveChatContext, ActiveChatMeta, ChatEntryRow, PendingChatAttachment},
    ui::{icons, palette},
};

const LIST_WIDTH: f32 = 252.0;
const TRANSCRIPT_MAX_WIDTH: f32 = 780.0;
const NO_AGENT_VALUE: &str = "__no_agent__";
/// How close to the end the transcript must be scrolled to keep following new text.
const FOLLOW_THRESHOLD: f32 = 56.0;
const CONTEXT_PANEL_WIDTH: f32 = 300.0;
/// Items of a context group listed before "+N more".
const CONTEXT_GROUP_SHOWN: usize = 8;

/// The Chats canvas for the selected project.
pub(crate) fn chat_view() -> impl IntoElement {
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
                    )),
            ),
        )
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
        .px(px(32.0))
        .gap(px(12.0))
        .child(icons::chat_bubbles(30.0, palette::text_muted()))
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
            || {
                view()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .pt(px(6.0))
                    .child(provider_picker())
                    .child(agent_picker())
                    .child(sandbox_picker(
                        "chat-new-access",
                        store::chat_new_sandbox,
                        store::set_chat_new_sandbox,
                    ))
                    .child(when(store::chat_worktree_available, workspace_picker))
            },
        ))
        .child(when(
            || store::chat_new_workspace() == ChatWorkspace::Worktree,
            || {
                text("The agent gets a new branch from HEAD in a folder of its own, so your checkout stays as it is.")
                    .max_w(px(470.0))
                    .text_center()
                    .text_size(px(11.5))
                    .text_color(palette::text_muted())
            },
        ))
}

fn workspace_picker() -> impl IntoElement {
    styled_select()
        .test_id("chat-new-workspace")
        .accessibility_label("Work in")
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
        .trigger(picker_trigger("Work in", || {
            store::chat_new_workspace().label().to_owned()
        }))
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

fn provider_picker() -> impl IntoElement {
    styled_select()
        .test_id("chat-new-provider")
        .accessibility_label("Provider")
        .items(|| {
            store::chat_provider_options()
                .into_iter()
                .map(|p| SelectItem::new(p.label(), p.label()))
                .collect::<Vec<_>>()
        })
        .value(|| SelectValue::from(store::chat_new_provider().label().to_owned()))
        .on_change(|value: &Option<SharedString>| {
            let provider = ProviderKind::CLI_PROVIDERS
                .iter()
                .copied()
                .find(|p| value.as_deref() == Some(p.label()))
                .unwrap_or(ProviderKind::Claude);
            store::set_chat_new_provider(provider);
        })
        .trigger(picker_trigger("Provider", || {
            store::chat_new_provider().label().to_owned()
        }))
}

fn agent_picker() -> impl IntoElement {
    styled_select()
        .test_id("chat-new-agent")
        .accessibility_label("Agent")
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
        .trigger(picker_trigger("Agent", || {
            let selected = store::chat_new_agent();
            selected
                .and_then(|reference| {
                    store::chat_agent_options()
                        .into_iter()
                        .find(|(r, _)| *r == reference)
                        .map(|(_, name)| name)
                })
                .unwrap_or_else(|| "None".to_owned())
        }))
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
                            || store::selected_project_id().is_some(),
                            move || chat_composer(draft),
                        )),
                )
                .child(when(store::chat_context_open, context_panel)),
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
                    move || !confirming_delete.get(),
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
                            .child(icons::close_x(palette::text_muted()))
                            .into_element()
                    }
                })),
        )
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
        .accessibility_label("Context window")
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
                        .child(icons::sync_arrows(palette::text_muted())),
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
                        .child(icons::close_x(palette::text_muted())),
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
                    .text_color(palette::text_muted()),
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

    let window = usage.window_tokens;
    let used_slices: Vec<(usize, &factory_core::ChatContextCategory)> = usage
        .categories
        .iter()
        .filter(|c| c.kind == ChatContextKind::Used)
        .enumerate()
        .collect();
    let slice_color =
        |index: usize| palette::project_color(SLICE_COLORS[index % SLICE_COLORS.len()]);

    // The bar: what is in the window, then the compaction reserve; the rest is free.
    let mut bar = view()
        .test_id("chat-context-bar")
        .h(px(10.0))
        .w_full()
        .flex_row()
        .rounded_full()
        .overflow_hidden()
        .bg(palette::segmented_track_bg());
    if window > 0 {
        for (index, category) in &used_slices {
            bar = bar.child(
                view()
                    .h_full()
                    .w(relative(
                        (category.tokens as f32 / window as f32).clamp(0.0, 1.0),
                    ))
                    .bg(slice_color(*index)),
            );
        }
        for category in usage
            .categories
            .iter()
            .filter(|c| c.kind == ChatContextKind::Buffer)
        {
            bar = bar.child(
                view()
                    .h_full()
                    .w(relative(
                        (category.tokens as f32 / window as f32).clamp(0.0, 1.0),
                    ))
                    .bg(palette::border_color()),
            );
        }
    }

    let mut legend = view().gap(px(6.0));
    for (index, category) in &used_slices {
        legend = legend.child(context_legend_row(
            slice_color(*index),
            &category.name,
            category.tokens,
            window,
        ));
    }
    for category in usage
        .categories
        .iter()
        .filter(|c| matches!(c.kind, ChatContextKind::Buffer | ChatContextKind::Free))
    {
        let swatch = if category.kind == ChatContextKind::Buffer {
            palette::border_color()
        } else {
            palette::segmented_track_bg()
        };
        legend = legend.child(context_legend_row(
            swatch,
            &category.name,
            category.tokens,
            window,
        ));
    }

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
            context.error.clone(),
        ))
        .child(bar)
        .child(legend);
    if !deferred.is_empty() {
        body = body.child(
            text(format!(
                "Not in the window until the agent asks for them: {}.",
                deferred.join(", ")
            ))
            .text_size(px(11.0))
            .text_color(palette::text_muted()),
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
    notes.push(format!(
        "Measured {} after the latest reply.",
        match relative_time(usage.captured_at).as_str() {
            "now" => "just now".to_owned(),
            ago => format!("{ago} ago"),
        }
    ));
    body.child(
        text(notes.join(" "))
            .text_size(px(11.0))
            .text_color(palette::text_muted()),
    )
    .into_element()
}

const SLICE_COLORS: [ProjectColor; 6] = [
    ProjectColor::Blue,
    ProjectColor::Teal,
    ProjectColor::Purple,
    ProjectColor::Orange,
    ProjectColor::Green,
    ProjectColor::Rose,
];

fn context_summary(
    usage: &ChatContextUsage,
    updating: bool,
    error: Option<String>,
) -> impl IntoElement {
    let fraction = if usage.window_tokens == 0 {
        0.0
    } else {
        (usage.used_tokens as f32 / usage.window_tokens as f32).clamp(0.0, 1.0)
    };
    let mut detail = vec![format!("{}% full", usage.used_percent())];
    if let Some(model) = usage.model.as_ref() {
        detail.push(model.clone());
    }
    if updating {
        detail.push("updating…".to_owned());
    }
    view()
        .gap(px(3.0))
        .child(
            text(format!(
                "{} of {} tokens",
                compact_tokens(usage.used_tokens),
                compact_tokens(usage.window_tokens)
            ))
            .test_id("chat-context-total")
            .tabular_nums()
            .text_size(px(17.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette::text_primary()),
        )
        .child(
            text(detail.join(" · "))
                .text_size(px(11.5))
                .text_color(if fraction >= 0.7 {
                    fullness_color(fraction)
                } else {
                    palette::text_muted()
                }),
        )
        .child(match error {
            Some(message) => text(message)
                .text_size(px(11.0))
                .text_color(palette::destructive_text())
                .into_element(),
            None => view().into_element(),
        })
}

fn context_legend_row(
    swatch: AdaptiveColor,
    name: &str,
    tokens: u64,
    window: u64,
) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            view()
                .w(px(9.0))
                .h(px(9.0))
                .flex_shrink_0()
                .rounded(px(2.5))
                .border_1()
                .border_color(palette::divider_color())
                .bg(swatch),
        )
        .child(
            text(name.to_owned())
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_size(px(12.0))
                .text_color(palette::text_secondary()),
        )
        .child(
            text(compact_tokens(tokens))
                .tabular_nums()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_primary()),
        )
        .child(
            text(window_share(tokens, window))
                .w(px(42.0))
                .text_right()
                .tabular_nums()
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
        )
}

fn context_group(group: &factory_core::ChatContextGroup) -> impl IntoElement {
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
                        .text_color(palette::text_muted()),
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
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
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
        ChatEntry::Notice { .. } => notice_row(row).into_element(),
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

fn tool_call_row(row: ReadSignal<ChatEntryRow>) -> impl IntoElement {
    let tool = move || match row.get().entry {
        ChatEntry::Tool {
            verb,
            target,
            status,
            detail,
            ..
        } => (verb, target, status, detail),
        _ => (
            String::new(),
            String::new(),
            ChatToolStatus::Running,
            String::new(),
        ),
    };

    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .pl(px(34.0))
        .min_w(px(0.0))
        .child(dynamic(move || match tool().2 {
            ChatToolStatus::Running => icons::status_running(palette::link_blue()).into_element(),
            ChatToolStatus::Succeeded => icons::check(palette::success_text()).into_element(),
            ChatToolStatus::Failed => icons::close_x(palette::destructive_text()).into_element(),
        }))
        .child(
            text(move || tool().0)
                .flex_shrink_0()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::text_secondary()),
        )
        .child(
            text(move || tool().1)
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .app_class("font-mono")
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(when(
            move || {
                let (_, _, status, detail) = tool();
                status == ChatToolStatus::Failed && !detail.is_empty()
            },
            move || {
                text(move || tool().3)
                    .max_w(px(260.0))
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(palette::destructive_text())
            },
        ))
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
