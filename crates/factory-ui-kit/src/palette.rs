//! Adaptive light/dark color tokens matching the macOS Software Factory dashboard aesthetic.

use factory_types::ProjectColor;
use nsh_core::prelude::{AdaptiveColor, rgb, rgba};

pub fn canvas_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFAFAFB), rgb(0x111317))
}

pub fn header_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF6F7F9), rgb(0x16181D))
}

pub fn sidebar_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF2F3F6), rgb(0x16181D))
}

pub fn sidebar_hover_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE8EAEF), rgb(0x22262E))
}

pub fn card_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFFFFFF), rgb(0x1B1E24))
}

pub fn card_inset_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFCFCFD), rgb(0x20242C))
}

pub fn subtle_surface_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF2F3F6), rgb(0x232730))
}

pub fn segmented_track_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEAECEF), rgb(0x232730))
}

pub fn control_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFFFFFF), rgb(0x1E222A))
}

pub fn border_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE5E7EB), rgb(0x2C313B))
}

pub fn divider_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEFF0F3), rgb(0x252A33))
}

pub fn gridline_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF0F1F4), rgb(0x252A33))
}

pub fn text_primary() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x111827), rgb(0xF3F4F6))
}

pub fn text_secondary() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x374151), rgb(0xCBD5E1))
}

pub fn text_muted() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x6B7280), rgb(0x8B93A1))
}

/// Secondary text that must stay readable at small sizes: at least 6.3:1 on every surface
/// the chat context panel uses (panel, card, table header, badge, hover), in both modes.
/// `text_muted` sits right at 4.5:1 and drops below it on hover surfaces.
pub fn text_subdued() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x4B5563), rgb(0xA3ADBB))
}

pub fn nav_active_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x0B66D6), rgb(0x1C75E8))
}

pub fn nav_active_fg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFFFFFF), rgb(0xFFFFFF))
}

pub fn badge_orange_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDC5B24), rgb(0xE66A35))
}

pub fn warning_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB4532A), rgb(0xE88454))
}

pub fn warning_icon() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB87314), rgb(0xE5A03B))
}

pub fn success_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x128A60), rgb(0x34D399))
}

pub fn provider_bar_fill() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x8A909A), rgb(0x6F7785))
}

pub fn rank_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECEEF2), rgb(0x2B303A))
}

pub fn prefs_tab_active_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE6E8EC), rgb(0x262B35))
}

pub fn provider_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECEEF2), rgb(0x282D37))
}

pub fn provider_badge_selected_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgba(0xFFFFFF2E), rgba(0xFFFFFF2E))
}

pub fn destructive_text() -> AdaptiveColor {
    // At least 5:1 on light surfaces (the lighter 0xD93025 fell below 4.5:1 on them).
    AdaptiveColor::new(rgb(0xC5221F), rgb(0xF87171))
}

pub fn switch_off_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD1D5DB), rgb(0x3B4252))
}

pub fn intake_banner_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECF3FC), rgb(0x162235))
}

pub fn intake_banner_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDCE8F8), rgb(0x233856))
}

pub fn lane_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF2F3F6), rgb(0x171A20))
}

pub fn card_selected_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEAF2FD), rgb(0x18273E))
}

pub fn card_selected_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x1668D6), rgb(0x3B82F6))
}

pub fn link_blue() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x0B66D6), rgb(0x58A6FF))
}

pub fn tag_pill_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF4F5F7), rgb(0x252A34))
}

pub fn review_amber() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x9A6700), rgb(0xE3B341))
}

pub fn queue_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE5E7EB), rgb(0x2A2F3A))
}

pub fn status_running_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE7F1FD), rgb(0x182A45))
}

pub fn status_success_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE6F6EF), rgb(0x132E24))
}

pub fn status_warning_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF3E2), rgb(0x312412))
}

pub fn status_error_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFDECEC), rgb(0x35191A))
}

pub fn attempt_banner_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFDF3E7), rgb(0x2E2215))
}

pub fn attempt_banner_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF4DEC0), rgb(0x4A351E))
}

pub fn attempt_banner_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x9A5B13), rgb(0xF0B05C))
}

pub fn callout_amber_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF7EB), rgb(0x2B2316))
}

pub fn callout_amber_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF3E0BE), rgb(0x46381F))
}

pub fn diff_add_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECFDF3), rgb(0x122A1F))
}

pub fn diff_add_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x117A4B), rgb(0x4ADE80))
}

pub fn diff_del_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF2F2), rgb(0x331819))
}

pub fn diff_del_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xC82829), rgb(0xF87171))
}

pub fn provider_dot_green() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x16A34A), rgb(0x34D399))
}

pub fn run_dot_success_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x189A57), rgb(0x22B56B))
}

pub fn run_dot_running_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x1266D6), rgb(0x2574E8))
}

pub fn run_dot_failed_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xC83A32), rgb(0xF87171))
}

pub fn run_dot_empty_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD4D7DD), rgb(0x3A404D))
}

pub fn workflow_now_amber() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xC45B13), rgb(0xF09A49))
}

pub fn editor_unsaved_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF3E2), rgb(0x302313))
}

pub fn editor_unsaved_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xE8C088), rgb(0x7A5321))
}

pub fn editor_unsaved_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB46312), rgb(0xF0A84B))
}

pub fn editor_loop_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEEF5FE), rgb(0x132238))
}

pub fn editor_loop_divider() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD3E3FA), rgb(0x213A5E))
}

pub fn editor_loop_pill_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDCEBFC), rgb(0x193458))
}

pub fn editor_fallback_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF7EB), rgb(0x231D15))
}

pub fn editor_fallback_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDFB775), rgb(0x8C6429))
}

pub fn editor_connector_color() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xCBD2DC), rgb(0x3B4250))
}

pub fn editor_palette_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xECEDF1), rgb(0x272C35))
}

pub fn machine_badge_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEFF1F5), rgb(0x272C35))
}

pub fn machine_badge_selected_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF6FAFF), rgb(0x213452))
}

pub fn banner_info_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x7AB2EE), rgb(0x2E5C93))
}

pub fn pill_warning_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFEF8EE), rgb(0x312412))
}

pub fn pill_warning_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB88948), rgb(0x8C6429))
}

pub fn pill_warning_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x8F5B12), rgb(0xF0B05C))
}

pub fn pill_error_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFDF2F2), rgb(0x35191A))
}

pub fn pill_error_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xC95B55), rgb(0x9E3D39))
}

pub fn pill_error_text() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xB82E26), rgb(0xF87171))
}

pub fn table_header_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xF8F9FB), rgb(0x1E222A))
}

pub fn popover_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFFFFFF), rgb(0x1E222A))
}

pub fn popover_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD8DCE3), rgb(0x343A46))
}

pub fn menubar_overdue_dot() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xA6520B), rgb(0xE8843C))
}

pub fn menubar_fresh_dot() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0x1668D6), rgb(0x4C9AFF))
}

pub fn inbox_selected_muted_fg() -> AdaptiveColor {
    AdaptiveColor::new(rgba(0xFFFFFFD9), rgba(0xFFFFFFD9))
}

pub fn inbox_selected_pill_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgba(0xFFFFFF1A), rgba(0xFFFFFF1A))
}

pub fn inbox_selected_pill_border() -> AdaptiveColor {
    AdaptiveColor::new(rgba(0xFFFFFF5C), rgba(0xFFFFFF5C))
}

pub fn inbox_rule_box_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xFAFAFC), rgb(0x171A21))
}

pub fn inbox_rule_box_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xD4D8DF), rgb(0x343A47))
}

pub fn inbox_keycap_bg() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xEFEFF2), rgb(0x262B35))
}

pub fn inbox_keycap_border() -> AdaptiveColor {
    AdaptiveColor::new(rgb(0xDCDFE5), rgb(0x383E4C))
}

pub fn project_color(color: ProjectColor) -> AdaptiveColor {
    match color {
        ProjectColor::Blue => AdaptiveColor::new(rgb(0x2071E8), rgb(0x3B82F6)),
        ProjectColor::Orange => AdaptiveColor::new(rgb(0xE35B2B), rgb(0xF07040)),
        ProjectColor::Green => AdaptiveColor::new(rgb(0x12A373), rgb(0x22B986)),
        ProjectColor::Purple => AdaptiveColor::new(rgb(0x7C5CFC), rgb(0x9378FF)),
        ProjectColor::Teal => AdaptiveColor::new(rgb(0x0D9488), rgb(0x2DD4BF)),
        ProjectColor::Rose => AdaptiveColor::new(rgb(0xE11D48), rgb(0xFB7185)),
    }
}

/// The color of a context-window slice's icon (see [`crate::icons::ContextSlice`]).
pub fn context_slice_color(slice: crate::icons::ContextSlice) -> AdaptiveColor {
    use crate::icons::ContextSlice;
    match slice {
        ContextSlice::Prompt => project_color(ProjectColor::Purple),
        ContextSlice::Tools => project_color(ProjectColor::Teal),
        ContextSlice::McpTools => project_color(ProjectColor::Orange),
        ContextSlice::Agents => project_color(ProjectColor::Rose),
        ContextSlice::Memory => project_color(ProjectColor::Green),
        ContextSlice::Skills => AdaptiveColor::new(rgb(0xB7791F), rgb(0xF2B84B)),
        ContextSlice::Messages => project_color(ProjectColor::Blue),
        ContextSlice::Reply => AdaptiveColor::new(rgb(0x0891B2), rgb(0x22D3EE)),
        ContextSlice::Other => AdaptiveColor::new(rgb(0x64748B), rgb(0x94A3B8)),
        ContextSlice::Reserve | ContextSlice::Free => text_subdued(),
        ContextSlice::Input => project_color(ProjectColor::Blue),
        ContextSlice::Output => project_color(ProjectColor::Green),
        ContextSlice::CacheRead => project_color(ProjectColor::Purple),
        ContextSlice::CacheWrite => project_color(ProjectColor::Orange),
    }
}
