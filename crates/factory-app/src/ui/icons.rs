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
