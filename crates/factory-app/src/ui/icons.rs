//! Crisp vector SVG icons rendered natively via Native Shell's `svg` element.

use nsh_core::{element::ImageSrc, prelude::*};

fn icon_svg(markup: &'static str, size_px: f32) -> Svg {
    svg(ImageSrc::svg_markup(markup))
        .w(px(size_px))
        .h(px(size_px))
        .flex_shrink_0()
}

/// 2x2 rounded grid icon for the Dashboard sidebar item.
pub(crate) fn dashboard(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2" width="4.8" height="4.8" rx="1.1"/><rect x="9.2" y="2" width="4.8" height="4.8" rx="1.1"/><rect x="2" y="9.2" width="4.8" height="4.8" rx="1.1"/><rect x="9.2" y="9.2" width="4.8" height="4.8" rx="1.1"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Speech/inbox bubble icon for the Inbox sidebar item.
pub(crate) fn inbox(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 3.5h11a1 1 0 0 1 1 1v6.2a1 1 0 0 1-1 1H6.2l-3.2 2.3v-2.3h-.5a1 1 0 0 1-1-1V4.5a1 1 0 0 1 1-1Z"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Desktop monitor icon for the Machines sidebar item.
pub(crate) fn machines(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2.5" width="12" height="8.5" rx="1.3"/><path d="M5.5 13.5h5"/><path d="M8 11v2.5"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Left chevron `<` for period navigation.
pub(crate) fn chevron_left(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9.8 3.5 5.3 8l4.5 4.5"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Right chevron `>` for period navigation.
pub(crate) fn chevron_right(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6.2 3.5 10.7 8l-4.5 4.5"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Down chevron `▾` for dropdown triggers.
pub(crate) fn chevron_down(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M4.2 6.2 8 10l3.8-3.8"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Plus `+` icon for adding/registering a project.
pub(crate) fn plus(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M8 3.2v9.6M3.2 8h9.6"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Exclamation circle `(!)` icon for "Worth a look" insight callouts.
pub(crate) fn alert_circle(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6"/><path d="M8 5.1v3.4"/><circle cx="8" cy="11.1" r="0.6" fill="currentColor" stroke="none"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Gear / cog icon for Preferences General tab and sidebar Settings entry.
pub(crate) fn settings_gear(
    size_px: f32,
    color: impl IntoReactive<AdaptiveColor>,
) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><circle cx="10" cy="10" r="2.9"/><path d="M10 2.6v2.1M10 15.3v2.1M2.6 10h2.1M15.3 10h2.1M4.8 4.8l1.5 1.5M13.7 13.7l1.5 1.5M15.2 4.8l-1.5 1.5M6.3 13.7l-1.5 1.5"/></svg>"#,
        size_px,
    )
    .text_color(color)
}

/// Half-filled contrast circle for the Preferences Appearance tab.
pub(crate) fn appearance(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="10" cy="10" r="6.8"/><path d="M10 3.2A6.8 6.8 0 0 0 10 16.8Z" fill="currentColor" stroke="none"/></svg>"#,
        20.0,
    )
    .text_color(color)
}

/// Stacked provider / server bars icon for the Preferences Providers tab.
pub(crate) fn providers(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="14" height="5" rx="1.5"/><rect x="3" y="11" width="14" height="5" rx="1.5"/><circle cx="6.2" cy="6.5" r="0.8" fill="currentColor" stroke="none"/><circle cx="6.2" cy="13.5" r="0.8" fill="currentColor" stroke="none"/></svg>"#,
        20.0,
    )
    .text_color(color)
}

/// Larger desktop monitor icon for the Preferences Machines tab.
pub(crate) fn machines_toolbar(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><rect x="2.8" y="3.5" width="14.4" height="9.8" rx="1.6"/><path d="M6.8 16.5h6.4"/><path d="M10 13.3v3.2"/></svg>"#,
        20.0,
    )
    .text_color(color)
}

/// Puzzle piece icon for the Preferences Extensions tab.
pub(crate) fn extensions(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><path d="M7.5 4.2a1.7 1.7 0 0 1 3.4 0h3.3a1.3 1.3 0 0 1 1.3 1.3v3.3a1.7 1.7 0 0 1 0 3.4v3.3a1.3 1.3 0 0 1-1.3 1.3H5.5a1.3 1.3 0 0 1-1.3-1.3V5.5a1.3 1.3 0 0 1 1.3-1.3h2Z"/></svg>"#,
        20.0,
    )
    .text_color(color)
}

/// Horizontal sliders icon for the Preferences Advanced tab.
pub(crate) fn advanced(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round"><path d="M3.2 6h4.2M11 6h5.8M3.2 10h8.6M15.4 10h1.4M3.2 14h2.6M9.4 14h7.4"/><circle cx="9.2" cy="6" r="1.6"/><circle cx="13.6" cy="10" r="1.6"/><circle cx="7.6" cy="14" r="1.6"/></svg>"#,
        20.0,
    )
    .text_color(color)
}

/// Crisp checkmark icon for provider verification status ("Key works").
pub(crate) fn check(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3.2 8.4 6.5 11.7 12.8 4.8"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Small downward triangle caret for buttons like "Add Account ▾".
pub(crate) fn caret_down(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="currentColor"><path d="M3 4.3h6L6 8.3 3 4.3Z"/></svg>"#,
        10.0,
    )
    .text_color(color)
}

/// Sidebar toggle icon shown beside traffic lights in the toolbar.
pub(crate) fn sidebar_toggle(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2.8" width="12" height="10.4" rx="1.8"/><path d="M5.8 2.8v10.4"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Folder icon for project groups in the sidebar.
pub(crate) fn folder(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"><path d="M2 4.4a1.2 1.2 0 0 1 1.2-1.2h3.1l1.3 1.4h5.2A1.2 1.2 0 0 1 14 5.8v6a1.2 1.2 0 0 1-1.2 1.2H3.2A1.2 1.2 0 0 1 2 11.8V4.4Z"/></svg>"#,
        14.0,
    )
    .text_color(color)
}

/// Tray/queue icon for the Work queue sidebar item.
pub(crate) fn work_queue(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6"/><path d="M2.2 8.8h3.4l1.1 1.5h2.6l1.1-1.5h3.4"/></svg>"#,
        14.5,
    )
    .text_color(color)
}

/// Small workflow branch icon used in sidebar workflow rows and issue cards.
pub(crate) fn workflow_branch(
    size_px: f32,
    color: impl IntoReactive<AdaptiveColor>,
) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><circle cx="5" cy="4.2" r="1.7"/><circle cx="11" cy="11.8" r="1.7"/><path d="M5 5.9v2.2a3.7 3.7 0 0 0 3.7 3.7h.6"/></svg>"#,
        size_px,
    )
    .text_color(color)
}

/// Filter funnel icon for the intake rules banner.
pub(crate) fn funnel(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M2.4 3.2h11.2L9.4 8.2v4.1l-2.8 1.2V8.2L2.4 3.2Z"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Circular refresh icon for the Sync toolbar button.
pub(crate) fn sync_arrows(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M13 4.6A5.5 5.5 0 0 0 3.3 5.8"/><path d="M13 2.5v2.4h-2.4"/><path d="M3 11.4a5.5 5.5 0 0 0 9.7-1.2"/><path d="M3 13.5v-2.4h2.4"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Small filled play triangle for the Ad-hoc Run CTA button.
pub(crate) fn play_filled(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="currentColor"><path d="M3.2 2.4 9.6 6 3.2 9.6V2.4Z"/></svg>"#,
        11.0,
    )
    .text_color(color)
}

/// Open issue circle-dot indicator `⊙`.
pub(crate) fn issue_open(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4"><circle cx="8" cy="8" r="5.5"/><circle cx="8" cy="8" r="1.4" fill="currentColor" stroke="none"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Dashed circle indicator for queued issues waiting for an agent slot.
pub(crate) fn status_queued(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="2.4 2.2" stroke-linecap="round"><circle cx="8" cy="8" r="5.3"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Progress ring indicator for actively running agent tasks.
pub(crate) fn status_running(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><circle cx="8" cy="8" r="5.3" stroke-opacity="0.28"/><path d="M8 2.7a5.3 5.3 0 0 1 5.3 5.3"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Solid amber ring indicator for items paused at a human review gate.
pub(crate) fn status_review(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="8" cy="8" r="5.2"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Checkmark-in-circle indicator for completed and merged issues.
pub(crate) fn status_merged(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="5.5"/><path d="M5.6 8.2 7.2 9.8 10.6 6.3"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Magnifying glass icon for the command palette / search bar.
pub(crate) fn search(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="7.2" cy="7.2" r="4.5"/><path d="m10.7 10.7 2.8 2.8"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Pause icon `||` for the run control group.
pub(crate) fn pause(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M5.8 4.2v7.6M10.2 4.2v7.6"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Rounded stop square icon `□` for the run control group.
pub(crate) fn stop_square(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.55" stroke-linejoin="round"><rect x="4.2" y="4.2" width="7.6" height="7.6" rx="1.4"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Pencil icon for the Edit Workflow button.
pub(crate) fn pencil(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M11.3 2.7a1.5 1.5 0 0 1 2.1 2.1L5.8 12.4 2.7 13.3l.9-3.1 7.7-7.5Z"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Loop / repeat arrows icon for loop attempt badges.
pub(crate) fn loop_arrows(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7.5A4.8 4.8 0 0 1 11.4 4.3L13 5.8"/><path d="M13 3.2v2.6h-2.6"/><path d="M13 8.5a4.8 4.8 0 0 1-8.4 3.2L3 10.2"/><path d="M3 12.8v-2.6h2.6"/></svg>"#,
        12.0,
    )
    .text_color(color)
}

/// Shield icon for Needs approval step cards.
pub(crate) fn shield_approval(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2.2 13 4.2v3.6c0 3.1-2.1 5.1-5 6-2.9-.9-5-2.9-5-6V4.2L8 2.2Z"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Small terminal prompt `>_` inside a rounded frame for tool call cards.
pub(crate) fn terminal_prompt(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="2.8" width="12" height="10.4" rx="2"/><path d="m4.8 6.4 2 1.8-2 1.8"/><path d="M8.4 10h2.6"/></svg>"#,
        14.0,
    )
    .text_color(color)
}

/// Person / agent outline icon for the Inspector Agent section header.
pub(crate) fn user_agent(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="5.4" r="2.4"/><path d="M3.4 13.2a4.6 4.6 0 0 1 9.2 0"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Git branch icon for the Inspector Git section header.
pub(crate) fn git_branch(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="4.8" cy="3.8" r="1.6"/><circle cx="11.2" cy="5.2" r="1.6"/><circle cx="4.8" cy="12.2" r="1.6"/><path d="M4.8 5.4v5.2"/><path d="M11.2 6.8c0 2.2-1.8 3.2-6.4 3.2"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Clock / progress circle icon for Inspector Progress header and Run history sidebar row.
pub(crate) fn clock_progress(
    size_px: f32,
    color: impl IntoReactive<AdaptiveColor>,
) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="5.6"/><path d="M8 5v3.2l2.1 1.4"/></svg>"#,
        size_px,
    )
    .text_color(color)
}

/// Compact puzzle piece icon for extension-contributed tabs, cards, and sidebar widgets.
pub(crate) fn extension_puzzle(
    size_px: f32,
    color: impl IntoReactive<AdaptiveColor>,
) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"><path d="M6.2 3.3a1.3 1.3 0 0 1 2.6 0h2.4a1 1 0 0 1 1 1v2.4a1.3 1.3 0 0 1 0 2.6v2.4a1 1 0 0 1-1 1H4.8a1 1 0 0 1-1-1V4.3a1 1 0 0 1 1-1h1.4Z"/></svg>"#,
        size_px,
    )
    .text_color(color)
}
