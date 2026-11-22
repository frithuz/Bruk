//! Native Shell macOS entry point (`cargo run`).

#[cfg(target_os = "macos")]
fn main() -> impl std::process::Termination {
    use nsh_platform_macos as platform;

    env_logger::init();
    platform::run_app(
        platform::AppSource::Rust(platform::Root::of::<factory_app::App>()),
        platform::AppOptions {
            title: "Software Factory".to_owned(),
            size: (1120, 720),
            ..Default::default()
        },
    )
}

#[cfg(not(target_os = "macos"))]
compile_error!("factory-app is configured for macOS (AppKit via nsh-platform-macos).");
