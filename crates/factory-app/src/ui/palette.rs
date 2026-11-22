//! Adaptive light/dark color tokens matching the macOS Software Factory dashboard aesthetic.

use factory_core::ProjectColor;
use nsh_core::prelude::{AdaptiveColor, rgb};

pub(crate) fn canvas_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFAFAFB), rgb(0x111317))
}

pub(crate) fn header_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF6F7F9), rgb(0x16181D))
}

pub(crate) fn sidebar_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF2F3F6), rgb(0x16181D))
}

pub(crate) fn sidebar_hover_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE8EAEF), rgb(0x22262E))
}

pub(crate) fn card_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFFFFFF), rgb(0x1B1E24))
}

pub(crate) fn card_inset_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFCFCFD), rgb(0x20242C))
}

pub(crate) fn subtle_surface_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF2F3F6), rgb(0x232730))
}

pub(crate) fn segmented_track_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEAECEF), rgb(0x232730))
}

pub(crate) fn control_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFFFFFF), rgb(0x1E222A))
}

pub(crate) fn border_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE5E7EB), rgb(0x2C313B))
}

pub(crate) fn divider_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEFF0F3), rgb(0x252A33))
}

pub(crate) fn gridline_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF0F1F4), rgb(0x252A33))
}

pub(crate) fn text_primary() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x111827), rgb(0xF3F4F6))
}

pub(crate) fn text_secondary() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x374151), rgb(0xCBD5E1))
}

pub(crate) fn text_muted() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x6B7280), rgb(0x8B93A1))
}

pub(crate) fn nav_active_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x0B66D6), rgb(0x1C75E8))
}

pub(crate) fn nav_active_fg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFFFFFF), rgb(0xFFFFFF))
}

pub(crate) fn badge_orange_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDC5B24), rgb(0xE66A35))
}

pub(crate) fn warning_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB4532A), rgb(0xE88454))
}

pub(crate) fn warning_icon() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB87314), rgb(0xE5A03B))
}

pub(crate) fn success_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x128A60), rgb(0x34D399))
}

pub(crate) fn provider_bar_fill() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x8A909A), rgb(0x6F7785))
}

pub(crate) fn rank_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECEEF2), rgb(0x2B303A))
}

pub(crate) fn project_color(color: ProjectColor) -> AdaptiveColor {
    match color {
        ProjectColor::Blue => AdaptiveColor::new(rgb(0x2071E8), rgb(0x3B82F6)),
        ProjectColor::Orange => AdaptiveColor::new(rgb(0xE35B2B), rgb(0xF07040)),
        ProjectColor::Green => AdaptiveColor::new(rgb(0x12A373), rgb(0x22B986)),
        ProjectColor::Purple => AdaptiveColor::new(rgb(0x7C5CFC), rgb(0x9378FF)),
        ProjectColor::Teal => AdaptiveColor::new(rgb(0x0D9488), rgb(0x2DD4BF)),
        ProjectColor::Rose => AdaptiveColor::new(rgb(0xE11D48), rgb(0xFB7185)),
    }
}
