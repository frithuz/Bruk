//! Native Shell macOS entry point (`cargo run`).

#[cfg(target_os = "macos")]
fn main() -> impl std::process::Termination {
    use nsh_platform_macos as platform;

    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<factory_app::App>()),
        platform::AppOptions {
            title: "Bruk".to_owned(),
            icon: app_icon_path(),
            size: (1360, 920),
            ..Default::default()
        },
    )
}

#[cfg(target_os = "macos")]
fn app_icon_path() -> Option<std::path::PathBuf> {
    let manifest_icon = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("icon.png");
    if manifest_icon.is_file() {
        return Some(manifest_icon);
    }

    let cwd_icon = std::path::PathBuf::from("assets").join("icon.png");
    cwd_icon.is_file().then_some(cwd_icon)
}

#[cfg(not(target_os = "macos"))]
compile_error!("factory-app is configured for macOS (AppKit via nsh-platform-macos).");
