//! Embedded Atkinson Hyperlegible font families, theme font token helpers,
//! and native macOS Font Panel (`NSFontPanel` / Font Book collections) integration.

use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use factory_core::{
    AppearanceSettings, DEFAULT_MONO_FONT_FAMILY, DEFAULT_UI_FONT_FAMILY, FontPreference,
};
use nsh_core::{
    element::{FONT_MONO, FONT_SANS, Root, Tokens},
    prelude::*,
};

/// Which font role the native macOS Fonts panel is currently picking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontPickerTarget {
    Ui,
    Mono,
}

impl FontPickerTarget {
    #[cfg(target_os = "macos")]
    fn panel_title(self) -> &'static str {
        match self {
            Self::Ui => "UI Font",
            Self::Mono => "Code & Monospace Font",
        }
    }
}

/// All static OTF faces for `Atkinson Hyperlegible Next` (UI) and
/// `Atkinson Hyperlegible Mono` (code/monospace).
pub const EMBEDDED_FONTS: &[&[u8]] = &[
    // Atkinson Hyperlegible Next (UI)
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraLight.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraLightItalic.otf"
    ),
    include_bytes!("../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Light.otf"),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-LightItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Regular.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-RegularItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Medium.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-MediumItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-SemiBold.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-SemiBoldItalic.otf"
    ),
    include_bytes!("../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Bold.otf"),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-BoldItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraBold.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraBoldItalic.otf"
    ),
    // Atkinson Hyperlegible Mono (code & monospace)
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraLight.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraLightItalic.otf"
    ),
    include_bytes!("../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Light.otf"),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-LightItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Regular.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-RegularItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Medium.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-MediumItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-SemiBold.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-SemiBoldItalic.otf"
    ),
    include_bytes!("../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Bold.otf"),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-BoldItalic.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraBold.otf"
    ),
    include_bytes!(
        "../assets/fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraBoldItalic.otf"
    ),
];

/// Builds design tokens with `nsh-base`'s defaults plus the user's configured UI and monospace fonts.
pub fn tokens_for_appearance(appearance: &AppearanceSettings) -> Tokens {
    let mut tokens = nsh_base::Theme::default().into_tokens();
    if let Some(ui_font) = appearance.ui_font.resolve_ui() {
        tokens.set_font(FONT_SANS, ui_font);
    }
    if let Some(mono_font) = appearance.mono_font.resolve_mono() {
        tokens.set_font(FONT_MONO, mono_font);
    }
    tokens
}

/// Design tokens for the default appearance (Atkinson Hyperlegible Next + Mono).
pub fn default_tokens() -> Tokens {
    tokens_for_appearance(&AppearanceSettings::default())
}

/// Attaches all embedded Atkinson Hyperlegible font files and default font tokens to `root`
/// so they are registered synchronously before the first frame renders.
pub fn with_embedded_fonts(mut root: Root) -> Root {
    let span = crate::boot_log::span("boot.embed_fonts");
    let mut total_bytes = 0usize;
    for font in EMBEDDED_FONTS {
        total_bytes += font.len();
        root = root.font(*font);
    }
    let root = root.tokens(default_tokens());
    span.finish(format!(
        "count={} bytes={total_bytes}",
        EMBEDDED_FONTS.len()
    ));
    root
}

/// Applies the active font tokens for `appearance` to the running Native Shell surface.
pub(crate) fn apply_font_tokens(appearance: &AppearanceSettings) {
    set_tokens(tokens_for_appearance(appearance));
}

/// Normalizes a family name picked in the macOS Fonts panel into a [`FontPreference`].
pub(crate) fn normalize_picked_font(target: FontPickerTarget, family: &str) -> FontPreference {
    let trimmed = family.trim();
    if trimmed.is_empty() {
        return FontPreference::Default;
    }
    // Private macOS system UI font names (e.g. `.AppleSystemUIFont`, `.SFNSMono`) map to System.
    if trimmed.starts_with('.') {
        return FontPreference::System;
    }
    let default_family = match target {
        FontPickerTarget::Ui => DEFAULT_UI_FONT_FAMILY,
        FontPickerTarget::Mono => DEFAULT_MONO_FONT_FAMILY,
    };
    if trimmed.eq_ignore_ascii_case(default_family) {
        FontPreference::Default
    } else {
        FontPreference::Custom(trimmed.to_owned())
    }
}

#[derive(Default)]
struct FontPickQueueState {
    pending: VecDeque<(FontPickerTarget, String)>,
    waker: Option<Waker>,
}

/// Async queue delivering live font family picks from AppKit's main thread back to the `nsh` engine.
#[derive(Clone, Default)]
pub(crate) struct FontPickChannel {
    inner: Arc<Mutex<FontPickQueueState>>,
}

impl FontPickChannel {
    #[cfg(target_os = "macos")]
    fn push(&self, target: FontPickerTarget, family: String) {
        let waker = {
            let mut guard = self.inner.lock().expect("font pick channel poisoned");
            guard.pending.push_back((target, family));
            guard.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub(crate) fn next(&self) -> NextFontPick {
        NextFontPick {
            inner: self.inner.clone(),
        }
    }
}

pub(crate) struct NextFontPick {
    inner: Arc<Mutex<FontPickQueueState>>,
}

impl Future for NextFontPick {
    type Output = (FontPickerTarget, String);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.inner.lock().expect("font pick channel poisoned");
        if let Some(item) = guard.pending.pop_front() {
            Poll::Ready(item)
        } else {
            guard.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// Opens macOS's native Fonts panel (`NSFontPanel`, backed by Font Book collections) on the
/// main thread for `target`, preselecting `current_family` when available.
///
/// Live font picks are delivered to `channel`. Safely no-ops in headless tests where AppKit is
/// not running on the main thread.
pub(crate) fn open_font_panel(
    target: FontPickerTarget,
    current_family: Option<String>,
    channel: FontPickChannel,
) {
    #[cfg(target_os = "macos")]
    {
        drop(run_on_main_thread(move || {
            macos_panel::open(target, current_family.as_deref(), channel);
        }));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (target, current_family, channel);
    }
}

/// Closes macOS's shared Fonts panel if it is currently open.
pub(crate) fn close_font_panel() {
    #[cfg(target_os = "macos")]
    {
        drop(run_on_main_thread(macos_panel::close_if_open));
    }
}

#[cfg(target_os = "macos")]
mod macos_panel {
    use std::cell::RefCell;

    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
        runtime::AnyObject, sel,
    };
    use objc2_app_kit::{
        NSFont, NSFontChanging, NSFontManager, NSFontPanel, NSFontPanelModeMask, NSFontTraitMask,
        NSFontWeightRegular,
    };
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};

    use super::{FontPickChannel, FontPickerTarget};

    struct PickerState {
        target: FontPickerTarget,
        current_font: Option<Retained<NSFont>>,
        channel: Option<FontPickChannel>,
    }

    struct TargetIvars {
        state: RefCell<PickerState>,
    }

    define_class!(
        /// AppKit target receiving `changeFont:` actions from `NSFontManager` / `NSFontPanel`.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = TargetIvars]
        #[name = "BrukFontPanelTarget"]
        struct FontPanelTarget;

        unsafe impl NSObjectProtocol for FontPanelTarget {}

        unsafe impl NSFontChanging for FontPanelTarget {
            #[unsafe(method(changeFont:))]
            fn change_font(&self, sender: Option<&NSFontManager>) {
                let Some(manager) = sender else {
                    return;
                };
                let mtm = self.mtm();
                let mut state = self.ivars().state.borrow_mut();
                let base = state
                    .current_font
                    .clone()
                    .unwrap_or_else(|| default_panel_font(mtm, state.target));
                let converted = manager.convertFont(&base);
                let family = converted.familyName().map(|name| name.to_string());
                state.current_font = Some(converted);
                if let (Some(family), Some(channel)) = (family, state.channel.as_ref()) {
                    channel.push(state.target, family);
                }
            }

            #[unsafe(method(validModesForFontPanel:))]
            fn valid_modes_for_font_panel(&self, _font_panel: &NSFontPanel) -> NSFontPanelModeMask {
                NSFontPanelModeMask::ModeMaskCollection | NSFontPanelModeMask::ModeMaskFace
            }
        }
    );

    impl FontPanelTarget {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(TargetIvars {
                state: RefCell::new(PickerState {
                    target: FontPickerTarget::Ui,
                    current_font: None,
                    channel: None,
                }),
            });
            // SAFETY: `NSObject`'s designated initializer.
            unsafe { msg_send![super(this), init] }
        }
    }

    thread_local! {
        // Keep the weak `NSFontManager` target alive for the lifetime of the main thread.
        static SHARED_TARGET: RefCell<Option<Retained<FontPanelTarget>>> = const { RefCell::new(None) };
    }

    fn default_panel_font(_mtm: MainThreadMarker, target: FontPickerTarget) -> Retained<NSFont> {
        match target {
            FontPickerTarget::Ui => NSFont::systemFontOfSize(13.0),
            FontPickerTarget::Mono => {
                // SAFETY: `NSFontWeightRegular` is an immutable AppKit constant.
                let weight = unsafe { NSFontWeightRegular };
                NSFont::monospacedSystemFontOfSize_weight(13.0, weight)
            }
        }
    }

    fn resolve_initial_font(
        mtm: MainThreadMarker,
        manager: &NSFontManager,
        target: FontPickerTarget,
        family: Option<&str>,
    ) -> Retained<NSFont> {
        if let Some(first_family) = family
            .and_then(|list| list.split(',').next())
            .map(|name| name.trim().trim_matches(|c| c == '"' || c == '\''))
            .filter(|name| !name.is_empty())
        {
            let ns_family = NSString::from_str(first_family);
            if let Some(font) = manager.fontWithFamily_traits_weight_size(
                &ns_family,
                NSFontTraitMask::empty(),
                5,
                13.0,
            ) {
                return font;
            }
            if let Some(font) = NSFont::fontWithName_size(&ns_family, 13.0) {
                return font;
            }
        }
        default_panel_font(mtm, target)
    }

    pub(super) fn open(
        target_kind: FontPickerTarget,
        current_family: Option<&str>,
        channel: FontPickChannel,
    ) {
        // Headless tests execute `run_on_main_thread` on a worker thread where AppKit is inactive.
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };

        let target = SHARED_TARGET.with(|slot| {
            slot.borrow_mut()
                .get_or_insert_with(|| FontPanelTarget::new(mtm))
                .clone()
        });

        let manager = NSFontManager::sharedFontManager(mtm);
        let initial_font = resolve_initial_font(mtm, &manager, target_kind, current_family);
        {
            let mut state = target.ivars().state.borrow_mut();
            state.target = target_kind;
            state.current_font = Some(initial_font.clone());
            state.channel = Some(channel);
        }

        // SAFETY: `target` stays retained in `SHARED_TARGET` on the main thread and implements `changeFont:`.
        unsafe {
            let obj: &AnyObject = target.as_ref();
            manager.setTarget(Some(obj));
            manager.setAction(sel!(changeFont:));
        }
        manager.setSelectedFont_isMultiple(&initial_font, false);

        let panel = NSFontPanel::sharedFontPanel(mtm);
        panel.setWorksWhenModal(true);
        panel.setTitle(&NSString::from_str(target_kind.panel_title()));
        panel.setPanelFont_isMultiple(&initial_font, false);
        panel.orderFront(None);
        panel.makeKeyAndOrderFront(None);
    }

    pub(super) fn close_if_open() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if NSFontPanel::sharedFontPanelExists(mtm) {
            NSFontPanel::sharedFontPanel(mtm).close();
        }
    }
}

#[cfg(test)]
mod tests {
    use factory_core::{DEFAULT_MONO_FONT_FAMILY, DEFAULT_UI_FONT_FAMILY, FontPreference};

    use super::*;

    #[test]
    fn embedded_fonts_declare_expected_families() {
        assert_eq!(EMBEDDED_FONTS.len(), 28);
        for data in EMBEDDED_FONTS {
            assert!(data.starts_with(b"OTTO"), "expected OpenType CFF header");
        }

        let next_assets = [
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraLight.otf"),
            asset!(
                "fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraLightItalic.otf"
            ),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Light.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-LightItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Regular.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-RegularItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Medium.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-MediumItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-SemiBold.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-SemiBoldItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Bold.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-BoldItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraBold.otf"),
            asset!("fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-ExtraBoldItalic.otf"),
        ];
        for font in next_assets {
            assert_eq!(font.family, DEFAULT_UI_FONT_FAMILY);
        }

        let mono_assets = [
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraLight.otf"),
            asset!(
                "fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraLightItalic.otf"
            ),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Light.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-LightItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Regular.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-RegularItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Medium.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-MediumItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-SemiBold.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-SemiBoldItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Bold.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-BoldItalic.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraBold.otf"),
            asset!("fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-ExtraBoldItalic.otf"),
        ];
        for font in mono_assets {
            assert_eq!(font.family, DEFAULT_MONO_FONT_FAMILY);
        }
    }

    #[test]
    fn tokens_for_appearance_respect_default_system_and_custom_overrides() {
        let defaults = tokens_for_appearance(&AppearanceSettings::default());
        assert_eq!(
            defaults.font(FONT_SANS).map(|f| &**f),
            Some(DEFAULT_UI_FONT_FAMILY)
        );
        assert_eq!(
            defaults.font(FONT_MONO).map(|f| &**f),
            Some(DEFAULT_MONO_FONT_FAMILY)
        );

        let system = tokens_for_appearance(&AppearanceSettings {
            ui_font: FontPreference::System,
            mono_font: FontPreference::System,
            ..AppearanceSettings::default()
        });
        assert!(system.font(FONT_SANS).is_none());
        assert!(system.font(FONT_MONO).is_none());

        let custom = tokens_for_appearance(&AppearanceSettings {
            ui_font: FontPreference::Custom("  Inter  ".to_owned()),
            mono_font: FontPreference::Custom("JetBrains Mono".to_owned()),
            ..AppearanceSettings::default()
        });
        assert_eq!(custom.font(FONT_SANS).map(|f| &**f), Some("Inter"));
        assert_eq!(custom.font(FONT_MONO).map(|f| &**f), Some("JetBrains Mono"));
    }

    #[test]
    fn normalize_picked_font_maps_atkinson_system_and_custom_families() {
        assert_eq!(
            normalize_picked_font(FontPickerTarget::Ui, "Atkinson Hyperlegible Next"),
            FontPreference::Default
        );
        assert_eq!(
            normalize_picked_font(FontPickerTarget::Mono, "Atkinson Hyperlegible Mono"),
            FontPreference::Default
        );
        assert_eq!(
            normalize_picked_font(FontPickerTarget::Ui, ".AppleSystemUIFont"),
            FontPreference::System
        );
        assert_eq!(
            normalize_picked_font(FontPickerTarget::Mono, ".SFNSMono"),
            FontPreference::System
        );
        assert_eq!(
            normalize_picked_font(FontPickerTarget::Mono, "  Menlo  "),
            FontPreference::Custom("Menlo".to_owned())
        );
        assert_eq!(
            normalize_picked_font(FontPickerTarget::Ui, "   "),
            FontPreference::Default
        );
    }
}
