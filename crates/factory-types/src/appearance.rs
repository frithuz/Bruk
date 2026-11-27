//! Appearance, font, and project accent color types shared by preferences, fonts, and UI chrome.

use serde::{Deserialize, Serialize};

/// Color scheme preference applied to the desktop application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearanceMode {
    pub const ALL: &'static [AppearanceMode] = &[Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Default embedded UI font family name.
pub const DEFAULT_UI_FONT_FAMILY: &str = "Atkinson Hyperlegible Next";

/// Default embedded monospace font family name.
pub const DEFAULT_MONO_FONT_FAMILY: &str = "Atkinson Hyperlegible Mono";

/// User preference for either the UI or monospace font family.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "family", rename_all = "snake_case")]
pub enum FontPreference {
    /// Use the app's embedded Atkinson Hyperlegible font family.
    #[default]
    Default,
    /// Use the operating system's default UI or monospace font.
    System,
    /// Use a custom installed font family name (or comma-separated fallback list).
    Custom(String),
}

impl FontPreference {
    /// Resolves this preference to the `font-sans` token value (`None` uses the macOS system font).
    pub fn resolve_ui(&self) -> Option<&str> {
        self.resolve_with_default(DEFAULT_UI_FONT_FAMILY)
    }

    /// Resolves this preference to the `font-mono` token value (`None` uses the macOS system monospace font).
    pub fn resolve_mono(&self) -> Option<&str> {
        self.resolve_with_default(DEFAULT_MONO_FONT_FAMILY)
    }

    /// Returns the trimmed custom family name when [`Self::Custom`] is active.
    pub fn custom_family(&self) -> Option<&str> {
        match self {
            Self::Custom(name) => Some(name.trim()),
            Self::Default | Self::System => None,
        }
    }

    fn resolve_with_default<'a>(&'a self, default_family: &'a str) -> Option<&'a str> {
        match self {
            Self::Default => Some(default_family),
            Self::System => None,
            Self::Custom(name) => {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    Some(default_family)
                } else {
                    Some(trimmed)
                }
            }
        }
    }
}

/// Visual appearance preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub mode: AppearanceMode,
    pub compact_sidebar: bool,
    pub monospace_font_size: u8,
    #[serde(default)]
    pub ui_font: FontPreference,
    #[serde(default)]
    pub mono_font: FontPreference,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            mode: AppearanceMode::System,
            compact_sidebar: false,
            monospace_font_size: 12,
            ui_font: FontPreference::Default,
            mono_font: FontPreference::Default,
        }
    }
}

/// Accent color assigned to a project across the sidebar, charts, and tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectColor {
    Blue,
    Orange,
    Green,
    Purple,
    Teal,
    Rose,
}

impl ProjectColor {
    /// Deterministic palette color for a project name.
    pub fn for_project_name(name: &str) -> Self {
        match name {
            "api-gateway" => Self::Blue,
            "marketing-site" => Self::Orange,
            "mobile-app" => Self::Green,
            other => {
                let hash = other.bytes().fold(0usize, |acc, b| {
                    acc.wrapping_mul(31).wrapping_add(b as usize)
                });
                match hash % 6 {
                    0 => Self::Blue,
                    1 => Self::Orange,
                    2 => Self::Green,
                    3 => Self::Purple,
                    4 => Self::Teal,
                    _ => Self::Rose,
                }
            }
        }
    }
}
