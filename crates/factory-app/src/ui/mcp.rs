//! MCP server UI shared by Project Settings › MCP servers, Preferences › Extensions, chats,
//! workflow runs and the Inbox: the project's tracked servers and their approval, the user's own
//! servers, and the banner shown when a server doesn't reach an agent.

use factory_core::mcp::{McpProblem, McpServerDef, ProjectMcpFile, ProjectMcpState};
use nsh_base::{button, input};
use nsh_core::prelude::*;

use crate::{
    store::{self, McpServerScope},
    ui::{icons, palette},
};

// --- Problem banner ---------------------------------------------------------------------

/// A banner for an MCP server that didn't reach the agent: what went wrong and a button that
/// takes the user to the fix. Red when it stopped the step or turn, amber otherwise.
pub(crate) fn mcp_problem_banner(problem: McpProblem, project_name: Option<String>) -> View {
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
        "MCP server missing — stopped"
    } else {
        "MCP server missing"
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
                .test_id("mcp-problem-fix")
                .accessibility_label(label)
                .on_press(move || store::apply_mcp_fix(project_name.as_deref(), &fix))
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

// --- Project Settings › MCP servers -------------------------------------------------------

/// The MCP servers tab: the project's tracked file (with its approval) and the user's own
/// servers for this project.
pub(crate) fn project_mcp_tab() -> impl IntoElement {
    let form = AddServerForm::new();
    scroll().flex_1().bg(palette::canvas_bg()).child(
        view()
            .flex_1()
            .px(px(26.0))
            .py(px(22.0))
            .gap(px(18.0))
            .child(dynamic(move || {
                let overview = store::project_mcp_overview();
                view()
                    .gap(px(18.0))
                    .child(heading(
                        "MCP servers",
                        "Servers agents get in workflow steps and chats. The project's are tracked in the repository; yours are added on top, never in place of them. Workflows with `mcp_sources: project` only get the project's.",
                    ))
                    .child(project_file_card(&overview))
                    .child(servers_card(
                        "Your servers for this project",
                        "Only you get these, and only in this project. Use ${VAR} for secrets and set them in Environment.",
                        McpServerScope::Project,
                        &overview.project_extras,
                    ))
                    .child(form.view(McpServerScope::Project))
                    .child(messages(McpServerScope::Project))
            })),
    )
}

fn project_file_card(overview: &store::ProjectMcpOverview) -> View {
    let mut card = card().child(
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
                        text("Project servers")
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette::text_secondary()),
                    )
                    .child(
                        text(file_caption(&overview.state))
                            .app_class("font-mono")
                            .text_size(px(11.5))
                            .text_color(palette::text_muted()),
                    ),
            )
            .child(approval_control(overview)),
    );

    match &overview.state {
        ProjectMcpState::Absent => {
            card = card.child(divider()).child(note_row(
                "This project doesn't track any MCP servers. Add .agents/mcp_config.json (or .mcp.json) to the repository to give every developer's agents the same servers.",
            ));
        }
        ProjectMcpState::Invalid { message, .. } => {
            card = card.child(divider()).child(problem_row(&format!(
                "The file {message}. Fix it in the repository; until then none of its servers load."
            )));
        }
        ProjectMcpState::Loaded(config) => {
            if !overview.allow_project_servers {
                card = card.child(divider()).child(problem_row(
                    "Project MCP servers are turned off in Preferences › Extensions.",
                ));
            } else if overview.changed_since_approval {
                card = card.child(divider()).child(problem_row(
                    "The file changed since you approved it. Review the servers below, then approve them again.",
                ));
            } else if !overview.approved && config.servers.iter().any(|s| !s.disabled) {
                card = card.child(divider()).child(problem_row(
                    "These servers run commands from the repository. Review them, then approve to let agents use them.",
                ));
            }
            if config.file == ProjectMcpFile::RootMcpJson {
                card = card.child(divider()).child(note_row(
                    "Antigravity only reads .agents/mcp_config.json; move the servers there to use them with every provider.",
                ));
            }
            if config.servers.is_empty() && config.invalid.is_empty() {
                card = card
                    .child(divider())
                    .child(note_row("The file declares no servers."));
            }
            for server in &config.servers {
                card = card.child(divider()).child(server_row(server, None));
            }
            for entry in &config.invalid {
                card = card.child(divider()).child(problem_row(&format!(
                    "'{}' can't be used: {}",
                    entry.name, entry.reason
                )));
            }
        }
    }
    card
}

fn file_caption(state: &ProjectMcpState) -> String {
    match state.file() {
        Some(ProjectMcpFile::AgentsMcpConfig) => ".agents/mcp_config.json".to_owned(),
        Some(ProjectMcpFile::RootMcpJson) => {
            ".mcp.json (no .agents/mcp_config.json in the repository)".to_owned()
        }
        None => "No .agents/mcp_config.json or .mcp.json".to_owned(),
    }
}

fn approval_control(overview: &store::ProjectMcpOverview) -> AnyElement {
    let has_servers = overview
        .state
        .config()
        .is_some_and(|c| c.servers.iter().any(|s| !s.disabled));
    if !has_servers {
        return view().into_element();
    }
    if overview.approved {
        view()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(
                text("Approved")
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(palette::text_muted()),
            )
            .child(small_button(
                "ps-mcp-revoke",
                "Revoke",
                store::revoke_project_mcp_servers,
            ))
            .into_element()
    } else {
        button()
            .test_id("ps-mcp-approve")
            .accessibility_label("Approve Servers")
            .on_press(store::approve_project_mcp_servers)
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(7.0))
            .bg(palette::nav_active_bg())
            .active(|s| s.opacity(0.8))
            .child(
                text("Approve Servers")
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette::nav_active_fg()),
            )
            .into_element()
    }
}

// --- Preferences › Extensions -------------------------------------------------------------

/// The user's servers for every project, for Preferences › Extensions.
pub(crate) fn user_mcp_servers_section() -> impl IntoElement {
    let form = AddServerForm::new();
    view().gap(px(12.0)).child(dynamic(move || {
        let servers = store::user_mcp_servers();
        view()
            .gap(px(12.0))
            .child(servers_card(
                "Your MCP servers (all projects)",
                "Added to every project's agents, after the project's own servers. Workflows that only allow project servers don't get them, and Antigravity can't be given them.",
                McpServerScope::User,
                &servers,
            ))
            .child(form.view(McpServerScope::User))
            .child(messages(McpServerScope::User))
    }))
}

// --- Shared pieces -------------------------------------------------------------------------

fn servers_card(
    title: &'static str,
    subtitle: &'static str,
    scope: McpServerScope,
    servers: &[McpServerDef],
) -> View {
    let mut card = card().child(
        view()
            .px(px(16.0))
            .py(px(10.0))
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
    );
    if servers.is_empty() {
        card = card.child(divider()).child(note_row("None yet."));
    }
    for server in servers {
        card = card.child(divider()).child(server_row(server, Some(scope)));
    }
    card
}

fn server_row(server: &McpServerDef, editable: Option<McpServerScope>) -> View {
    let mut details = view()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(2.0))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    text(server.name.clone())
                        .app_class("font-mono")
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if server.disabled {
                            palette::text_muted()
                        } else {
                            palette::text_primary()
                        }),
                )
                .child(
                    text(if server.disabled {
                        format!("{} · off", server.transport.kind_label())
                    } else {
                        server.transport.kind_label().to_owned()
                    })
                    .text_size(px(10.5))
                    .text_color(palette::text_muted()),
                ),
        )
        .child(
            text(server.transport.summary())
                .app_class("font-mono")
                .truncate()
                .text_size(px(11.5))
                .text_color(palette::text_secondary()),
        );
    let refs = server.env_references();
    if !refs.is_empty() {
        details = details.child(
            text(format!("Needs {}", refs.join(", ")))
                .text_size(px(11.0))
                .text_color(palette::text_muted()),
        );
    }
    let mut row = view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(9.0))
        .child(details);
    if let Some(scope) = editable {
        let toggle_name = server.name.clone();
        let remove_name = server.name.clone();
        row = row
            .child(small_button(
                "mcp-server-toggle",
                if server.disabled {
                    "Turn On"
                } else {
                    "Turn Off"
                },
                move || store::toggle_mcp_server(scope, &toggle_name),
            ))
            .child(
                button()
                    .test_id(&format!("mcp-server-remove-{}", server.name))
                    .accessibility_label(format!("Remove {}", server.name))
                    .on_press(move || store::remove_mcp_server(scope, &remove_name))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(5.0))
                    .hover(|s| s.bg(palette::subtle_surface_bg()))
                    .child(
                        text("Remove")
                            .text_size(px(11.5))
                            .text_color(palette::destructive_text()),
                    ),
            );
    }
    row
}

/// The add-server form's drafts, kept across re-renders of the list above it.
#[derive(Clone, Copy)]
struct AddServerForm {
    open: Signal<bool>,
    name: Signal<String>,
    target: Signal<String>,
    args: Signal<String>,
    pairs: Signal<String>,
}

impl AddServerForm {
    fn new() -> Self {
        Self {
            open: signal(false),
            name: signal(String::new()),
            target: signal(String::new()),
            args: signal(String::new()),
            pairs: signal(String::new()),
        }
    }

    fn clear(&self) {
        self.name.set(String::new());
        self.target.set(String::new());
        self.args.set(String::new());
        self.pairs.set(String::new());
    }

    /// The form, or its Add Server button. Only opening and closing re-renders it, so typing
    /// keeps the inputs (and their focus).
    fn view(self, scope: McpServerScope) -> impl IntoElement {
        dynamic(move || self.render(scope))
    }

    fn render(self, scope: McpServerScope) -> AnyElement {
        let form = self;
        if !self.open.get() {
            return view()
                .flex_row()
                .child(small_button("mcp-add-server", "Add Server", move || {
                    form.clear();
                    form.open.set(true);
                }))
                .into_element();
        }
        let save = move || {
            if store::add_mcp_server(
                scope,
                &form.name.get_untracked(),
                &form.target.get_untracked(),
                &form.args.get_untracked(),
                &form.pairs.get_untracked(),
            ) {
                form.clear();
                form.open.set(false);
            }
        };
        card()
            .p(px(14.0))
            .gap(px(10.0))
            .child(
                view()
                    .flex_row()
                    .gap(px(10.0))
                    .child(field(
                        "Name",
                        "e.g. playwright",
                        "mcp-form-name",
                        self.name,
                        false,
                    ))
                    .child(field(
                        "Command or URL",
                        "npx, or https://…/mcp",
                        "mcp-form-target",
                        self.target,
                        false,
                    )),
            )
            .child(field(
                "Arguments (commands only)",
                "-y @playwright/mcp@latest",
                "mcp-form-args",
                self.args,
                false,
            ))
            .child(field(
                "Environment, one KEY=value per line (URL servers: one 'Header: value' per line)",
                "TOKEN=${MY_TOKEN}",
                "mcp-form-pairs",
                self.pairs,
                true,
            ))
            .child(
                view()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .child(small_button("mcp-form-cancel", "Cancel", move || {
                        form.open.set(false);
                    }))
                    .child(
                        button()
                            .test_id("mcp-form-save")
                            .accessibility_label("Add Server")
                            .on_press(save)
                            .px(px(12.0))
                            .py(px(6.0))
                            .rounded(px(7.0))
                            .bg(palette::nav_active_bg())
                            .active(|s| s.opacity(0.8))
                            .child(
                                text("Add Server")
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(palette::nav_active_fg()),
                            ),
                    ),
            )
            .into_element()
    }
}

fn field(
    label: &'static str,
    placeholder: &'static str,
    test_id: &'static str,
    value: Signal<String>,
    multiline: bool,
) -> impl IntoElement {
    let mut input = input()
        .test_id(test_id)
        .value(move || value.get())
        .on_change(move |v: &str| value.set(v.to_owned()))
        .placeholder(placeholder)
        .app_class("font-mono")
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_inset_bg())
        .text_size(px(12.5))
        .text_color(palette::text_primary());
    if multiline {
        input = input.multiline(true).h(px(64.0));
    }
    view()
        .flex_1()
        .gap(px(4.0))
        .child(
            text(label)
                .text_size(px(11.5))
                .text_color(palette::text_muted()),
        )
        .child(input)
}

fn messages(scope: McpServerScope) -> impl IntoElement {
    let (error, feedback) = match scope {
        McpServerScope::Project => (
            store::project_settings_error(),
            store::project_settings_feedback(),
        ),
        McpServerScope::User => (store::preferences_error(), store::preferences_feedback()),
    };
    let mut strip = view().gap(px(4.0));
    if let Some(error) = error {
        strip = strip.child(
            text(error)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette::destructive_text()),
        );
    }
    if let Some(feedback) = feedback {
        strip = strip.child(
            text(feedback)
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        );
    }
    strip
}

fn heading(title: &'static str, subtitle: &'static str) -> impl IntoElement {
    view()
        .gap(px(3.0))
        .child(
            text(title)
                .text_size(px(14.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette::text_primary()),
        )
        .child(
            text(subtitle)
                .text_size(px(12.0))
                .text_color(palette::text_muted()),
        )
}

fn card() -> View {
    view()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette::border_color())
        .bg(palette::card_bg())
        .overflow_hidden()
}

fn divider() -> impl IntoElement {
    view().h(px(1.0)).w_full().bg(palette::divider_color())
}

fn note_row(message: &str) -> impl IntoElement {
    view().px(px(16.0)).py(px(10.0)).child(
        text(message.to_owned())
            .text_size(px(12.0))
            .text_color(palette::text_muted()),
    )
}

fn problem_row(message: &str) -> impl IntoElement {
    view()
        .flex_row()
        .items_start()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(10.0))
        .bg(palette::pill_warning_bg())
        .child(icons::alert_circle(palette::pill_warning_text()))
        .child(
            text(message.to_owned())
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(12.0))
                .text_color(palette::text_primary()),
        )
}

fn small_button(
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
