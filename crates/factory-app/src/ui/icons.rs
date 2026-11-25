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

/// Gear / cog icon for Preferences General tab and project settings entry.
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

/// Key icon for the Preferences Secrets tab.
pub(crate) fn secrets(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><circle cx="7.2" cy="10" r="3.4"/><path d="M10.6 10h6.4"/><path d="M14.2 10v2.4"/><path d="M16.8 10v1.8"/></svg>"#,
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

/// Three horizontal dots `•••` for overflow action buttons.
pub(crate) fn more_horizontal(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="currentColor"><circle cx="3.5" cy="8" r="1.25"/><circle cx="8" cy="8" r="1.25"/><circle cx="12.5" cy="8" r="1.25"/></svg>"#,
        14.0,
    )
    .text_color(color)
}

/// Crisp white checkmark sized for a 14×14 succeeded run dot.
pub(crate) fn run_dot_check(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2.6 6.3 5 8.7 9.4 3.6"/></svg>"#,
        10.0,
    )
    .text_color(color)
}

/// Crisp `×` mark sized for a 14×14 failed/stopped run dot.
pub(crate) fn run_dot_cross(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M3.4 3.4 8.6 8.6M8.6 3.4 3.4 8.6"/></svg>"#,
        10.0,
    )
    .text_color(color)
}

/// Three tiny horizontal dots sized for a 14×14 running/waiting run dot.
pub(crate) fn run_dot_ellipsis(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12" fill="currentColor"><circle cx="2.7" cy="6" r="1.05"/><circle cx="6" cy="6" r="1.05"/><circle cx="9.3" cy="6" r="1.05"/></svg>"#,
        10.0,
    )
    .text_color(color)
}

/// Three vertical stacked module bars for Composite entries in the workflow editor palette.
pub(crate) fn composite_grid(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"><rect x="2.4" y="3.2" width="11.2" height="9.6" rx="1.6"/><path d="M6.2 3.2v9.6M9.8 3.2v9.6"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Vertical blue return-arrow rail connecting the bottom step of a loop back to the first step.
pub(crate) fn loop_return_arrow(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    svg(ImageSrc::svg_markup(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 22 108" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round"><path d="M18 89H10a4 4 0 0 1-4-4V21a4 4 0 0 1 4-4h8"/><path d="M14.2 13.2 18.2 17l-4 3.8"/></svg>"#,
    ))
    .w(px(22.0))
    .h(px(108.0))
    .flex_shrink_0()
    .text_color(color)
}

/// Crisp small close `×` icon for removing optional exit conditions or closing drawers.
pub(crate) fn close_x(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M3.8 3.8 10.2 10.2M10.2 3.8 3.8 10.2"/></svg>"#,
        11.5,
    )
    .text_color(color)
}

/// Downward arrow into a tray line for the `Update Cores...` button.
pub(crate) fn download_tray(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2.8v7.4"/><path d="M5.2 7.5 8 10.3l2.8-2.8"/><path d="M3.2 12.8h9.6"/></svg>"#,
        13.5,
    )
    .text_color(color)
}

/// Info `i` inside a circle for catch-up and informational banners.
pub(crate) fn info_circle(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="6"/><path d="M8 7.3v3.5"/><circle cx="8" cy="5.1" r="0.65" fill="currentColor" stroke="none"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Broken link / disconnected indicator for disconnected remote machines.
pub(crate) fn status_disconnected(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><path d="M6.4 9.6 4.7 11.3a2.2 2.2 0 1 1-3.1-3.1l1.7-1.7"/><path d="M9.6 6.4 11.3 4.7a2.2 2.2 0 1 1 3.1 3.1l-1.7 1.7"/><path d="M6.2 4.3 5.7 2.8M11.7 6.2l1.5-.5M4.3 9.8l-1.5.5M9.8 11.7l.5 1.5"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Pause bars inside a circle for machines that are connected but not syncing.
pub(crate) fn status_not_syncing(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="5.6"/><path d="M6.6 5.9v4.2M9.4 5.9v4.2"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Cross `×` inside a circle for unreachable machines.
pub(crate) fn status_unreachable(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="5.6"/><path d="M5.9 5.9 10.1 10.1M10.1 5.9 5.9 10.1"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Padlock icon for permission requests in the Inbox list.
pub(crate) fn lock_permission(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><rect x="3.4" y="7.2" width="9.2" height="6.4" rx="1.5"/><path d="M5.4 7.2V5.3a2.6 2.6 0 0 1 5.2 0v1.9"/><circle cx="8" cy="10.4" r="0.85" fill="currentColor" stroke="none"/></svg>"#,
        14.0,
    )
    .text_color(color)
}

/// Speech bubble with question mark for agent question requests in the Inbox list.
pub(crate) fn question_bubble(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M2.6 3.6h10.8a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H6.4l-2.8 2.2v-2.2h-1a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1Z"/><path d="M6.8 6.4a1.25 1.25 0 0 1 2.4.4c0 .8-.9 1.1-1.2 1.6"/><circle cx="8" cy="10" r="0.55" fill="currentColor" stroke="none"/></svg>"#,
        14.0,
    )
    .text_color(color)
}

/// Globe icon for network-access permission risk pills.
pub(crate) fn globe(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="5.6"/><ellipse cx="8" cy="8" rx="2.6" ry="5.6"/><path d="M2.6 8h10.8"/></svg>"#,
        12.5,
    )
    .text_color(color)
}

/// Two overlapping speech bubbles for Chats.
pub(crate) fn chat_bubbles(
    size_px: f32,
    color: impl IntoReactive<AdaptiveColor>,
) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M2 3.6a1.1 1.1 0 0 1 1.1-1.1h6.3a1.1 1.1 0 0 1 1.1 1.1v3.9a1.1 1.1 0 0 1-1.1 1.1H5.4L3.2 10.2V8.6h-.1A1.1 1.1 0 0 1 2 7.5Z"/><path d="M12.4 5.6h.5a1.1 1.1 0 0 1 1.1 1.1v3.7a1.1 1.1 0 0 1-1.1 1.1h-.1v1.6l-2.2-1.6H7.6a1.1 1.1 0 0 1-1.1-1.1v-.3"/></svg>"#,
        size_px,
    )
    .text_color(color)
}

/// Upward arrow for sending a chat message.
pub(crate) fn arrow_up(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M8 13V3.5"/><path d="M4 7.3 8 3.3l4 4"/></svg>"#,
        13.0,
    )
    .text_color(color)
}

/// Paper clip icon for attaching files to a chat message.
pub(crate) fn paperclip(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.45" stroke-linecap="round" stroke-linejoin="round"><path d="M13.2 7.6 8.1 12.7a3.2 3.2 0 0 1-4.5-4.5l5.4-5.4a2.1 2.1 0 0 1 3 3L6.6 11.2a1.05 1.05 0 0 1-1.5-1.5l4.8-4.8"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// Document icon for an attached file that isn't an image.
pub(crate) fn file_doc(color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    icon_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.2 1.9H4.6a1.2 1.2 0 0 0-1.2 1.2v9.8a1.2 1.2 0 0 0 1.2 1.2h6.8a1.2 1.2 0 0 0 1.2-1.2V5.3Z"/><path d="M9.2 1.9v3.4h3.4"/></svg>"#,
        15.0,
    )
    .text_color(color)
}

/// What a slice of a chat's context window holds, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextSlice {
    Prompt,
    Tools,
    McpTools,
    Agents,
    Memory,
    Skills,
    Messages,
    Reply,
    Reserve,
    Free,
    Other,
    /// Token kinds: fresh prompt input, output, and prompt-cache reads and writes.
    Input,
    Output,
    CacheRead,
    CacheWrite,
}

/// The icon of a context-window slice: a page for the system prompt, a wrench for tools, a
/// plug for MCP tools, a person for agents, a book for memory files, a sparkle for skills,
/// a speech bubble for messages, a reply arrow, a box for the compaction reserve, and a
/// dashed ring for free space.
pub(crate) fn context_slice(
    slice: ContextSlice,
    color: impl IntoReactive<AdaptiveColor>,
) -> impl IntoElement {
    let markup = match slice {
        ContextSlice::Prompt => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9.2 1.9H4.6a1.2 1.2 0 0 0-1.2 1.2v9.8a1.2 1.2 0 0 0 1.2 1.2h6.8a1.2 1.2 0 0 0 1.2-1.2V5.3Z"/><path d="M9.2 1.9v3.4h3.4"/><path d="M5.6 8.4h4.8"/><path d="M5.6 11h3.2"/></svg>"#
        }
        ContextSlice::Tools => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M10.4 2.2a3.3 3.3 0 0 0-3.8 4.3l-4.1 4.1a1.35 1.35 0 0 0 1.9 1.9l4.1-4.1a3.3 3.3 0 0 0 4.3-3.8l-2.1 2.1-1.9-.4-.4-1.9Z"/></svg>"#
        }
        ContextSlice::McpTools => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 1.8v3.2"/><path d="M10 1.8v3.2"/><path d="M4.3 5h7.4v2.4a3.7 3.7 0 0 1-7.4 0Z"/><path d="M8 11.1v3.1"/></svg>"#
        }
        ContextSlice::Agents => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="5.4" r="2.7"/><path d="M2.9 13.8a5.1 5.1 0 0 1 10.2 0"/></svg>"#
        }
        ContextSlice::Memory => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3.4 12.9V3.1a1.1 1.1 0 0 1 1.1-1.1h8.1v9.8H4.5a1.1 1.1 0 0 0-1.1 1.1Zm0 0a1.1 1.1 0 0 0 1.1 1.1h8.1"/><path d="M6.2 5h3.6"/></svg>"#
        }
        ContextSlice::Skills => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 1.9 9.4 6.6 14.1 8 9.4 9.4 8 14.1 6.6 9.4 1.9 8 6.6 6.6Z"/></svg>"#
        }
        ContextSlice::Messages => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 3.5h11a1 1 0 0 1 1 1v6.2a1 1 0 0 1-1 1H6.2l-3.2 2.3v-2.3h-.5a1 1 0 0 1-1-1V4.5a1 1 0 0 1 1-1Z"/></svg>"#
        }
        ContextSlice::Reply => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6.4 3.6 2.6 7.4l3.8 3.8"/><path d="M2.6 7.4h7a3.8 3.8 0 0 1 3.8 3.8v1.2"/></svg>"#
        }
        ContextSlice::Reserve => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="1.9" y="2.8" width="12.2" height="3.2" rx=".9"/><path d="M3 6v6.4a1.1 1.1 0 0 0 1.1 1.1h7.8a1.1 1.1 0 0 0 1.1-1.1V6"/><path d="M6.4 9h3.2"/></svg>"#
        }
        ContextSlice::Free => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="8" cy="8" r="5.6" stroke-dasharray="2.3 2.1"/></svg>"#
        }
        ContextSlice::Other => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="3.4" fill="currentColor"/></svg>"#
        }
        ContextSlice::Input => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2v7.4"/><path d="M4.9 6.4 8 9.5l3.1-3.1"/><path d="M2.6 10.6v1.7a1.4 1.4 0 0 0 1.4 1.4h8a1.4 1.4 0 0 0 1.4-1.4v-1.7"/></svg>"#
        }
        ContextSlice::Output => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M8 9.6V2.2"/><path d="M4.9 5.3 8 2.2l3.1 3.1"/><path d="M2.6 10.6v1.7a1.4 1.4 0 0 0 1.4 1.4h8a1.4 1.4 0 0 0 1.4-1.4v-1.7"/></svg>"#
        }
        ContextSlice::CacheRead => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="7" cy="3.8" rx="4.6" ry="1.8"/><path d="M2.4 3.8v7.6c0 1 2 1.8 4.6 1.8"/><path d="M11.6 3.8v3"/><path d="M2.4 7.6c0 1 2 1.8 4.6 1.8"/><path d="M10 11.6h4.2"/><path d="M12.4 9.8l1.8 1.8-1.8 1.8"/></svg>"#
        }
        ContextSlice::CacheWrite => {
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="7" cy="3.8" rx="4.6" ry="1.8"/><path d="M2.4 3.8v7.6c0 1 2 1.8 4.6 1.8"/><path d="M11.6 3.8v3"/><path d="M2.4 7.6c0 1 2 1.8 4.6 1.8"/><path d="M12.2 9.4v4.4"/><path d="M10 11.6h4.4"/></svg>"#
        }
    };
    icon_svg(markup, 14.0).text_color(color)
}
