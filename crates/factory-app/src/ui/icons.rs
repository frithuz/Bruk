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
