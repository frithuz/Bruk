//! Adaptive light/dark color tokens matching the macOS Software Factory dashboard aesthetic.

use factory_core::ProjectColor;
use nsh_core::prelude::{AdaptiveColor, rgb, rgba};

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

pub(crate) fn prefs_tab_active_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE6E8EC), rgb(0x262B35))
}

pub(crate) fn provider_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECEEF2), rgb(0x282D37))
}

pub(crate) fn provider_badge_selected_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgba(0xFFFFFF2E), rgba(0xFFFFFF2E))
}

pub(crate) fn destructive_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD93025), rgb(0xF87171))
}

pub(crate) fn switch_off_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD1D5DB), rgb(0x3B4252))
}

pub(crate) fn intake_banner_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECF3FC), rgb(0x162235))
}

pub(crate) fn intake_banner_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDCE8F8), rgb(0x233856))
}

pub(crate) fn lane_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF2F3F6), rgb(0x171A20))
}

pub(crate) fn card_selected_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEAF2FD), rgb(0x18273E))
}

pub(crate) fn card_selected_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x1668D6), rgb(0x3B82F6))
}

pub(crate) fn link_blue() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x0B66D6), rgb(0x58A6FF))
}

pub(crate) fn tag_pill_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF4F5F7), rgb(0x252A34))
}

pub(crate) fn review_amber() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x9A6700), rgb(0xE3B341))
}

pub(crate) fn queue_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE5E7EB), rgb(0x2A2F3A))
}

pub(crate) fn status_running_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE7F1FD), rgb(0x182A45))
}

pub(crate) fn status_success_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE6F6EF), rgb(0x132E24))
}

pub(crate) fn status_warning_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF3E2), rgb(0x312412))
}

pub(crate) fn status_error_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFDECEC), rgb(0x35191A))
}

pub(crate) fn attempt_banner_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFDF3E7), rgb(0x2E2215))
}

pub(crate) fn attempt_banner_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF4DEC0), rgb(0x4A351E))
}

pub(crate) fn attempt_banner_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x9A5B13), rgb(0xF0B05C))
}

pub(crate) fn callout_amber_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF7EB), rgb(0x2B2316))
}

pub(crate) fn callout_amber_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF3E0BE), rgb(0x46381F))
}

pub(crate) fn diff_add_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECFDF3), rgb(0x122A1F))
}

pub(crate) fn diff_add_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x117A4B), rgb(0x4ADE80))
}

pub(crate) fn diff_del_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF2F2), rgb(0x331819))
}

pub(crate) fn diff_del_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xC82829), rgb(0xF87171))
}

pub(crate) fn provider_dot_green() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x16A34A), rgb(0x34D399))
}

pub(crate) fn provider_dot_blue() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x1668D6), rgb(0x58A6FF))
}

pub(crate) fn run_dot_success_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x189A57), rgb(0x22B56B))
}

pub(crate) fn run_dot_running_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x1266D6), rgb(0x2574E8))
}

pub(crate) fn run_dot_failed_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xC83A32), rgb(0xF87171))
}

pub(crate) fn run_dot_empty_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD4D7DD), rgb(0x3A404D))
}

pub(crate) fn workflow_now_amber() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xC45B13), rgb(0xF09A49))
}

pub(crate) fn editor_unsaved_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF3E2), rgb(0x302313))
}

pub(crate) fn editor_unsaved_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE8C088), rgb(0x7A5321))
}

pub(crate) fn editor_unsaved_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB46312), rgb(0xF0A84B))
}

pub(crate) fn editor_loop_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEEF5FE), rgb(0x132238))
}

pub(crate) fn editor_loop_divider() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD3E3FA), rgb(0x213A5E))
}

pub(crate) fn editor_loop_pill_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDCEBFC), rgb(0x193458))
}

pub(crate) fn editor_fallback_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF7EB), rgb(0x231D15))
}

pub(crate) fn editor_fallback_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDFB775), rgb(0x8C6429))
}

pub(crate) fn editor_connector_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xCBD2DC), rgb(0x3B4250))
}

pub(crate) fn editor_palette_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECEDF1), rgb(0x272C35))
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
