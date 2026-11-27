#![allow(dead_code)]

use std::{ffi::OsString, path::PathBuf};

use nsh_core::{
    platform::Os,
    prelude::{Size, px},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;

pub fn start_with_db(db_path: PathBuf) -> Headless {
    start_with_db_and_claude_path(db_path, OsString::new())
}

pub fn start_with_db_and_claude_path(db_path: PathBuf, path_env: OsString) -> Headless {
    let mut config = EngineConfig::new(factory_app::root_with_db_path_and_claude_path(
        db_path, path_env,
    ));
    config.environment.os = Os::MacOs;
    config.viewport = Size::new(px(1360.0), px(920.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("Dashboard").unwrap();
    app
}

/// Waits for the launch-time provider probes to settle, then presses a provider row in
/// Preferences until that provider's detail pane shows `pane`.
///
/// Provider CLI probes finish on background threads after launch and rebuild Preferences.
/// `Headless::press` dispatches to the node it found on its last screen snapshot, so a probe that
/// lands mid-press swaps that node out and the press is silently dropped (a real pointer would
/// hit-test the new row). Selecting a provider is idempotent, so pressing again is safe.
pub fn select_provider(app: &mut Headless, row: &str, pane: &str) {
    app.wait_for(row).unwrap();
    // Let the launch probes and the detail refreshes they trigger land before pressing anything.
    app.idle(std::time::Duration::from_millis(750)).unwrap();
    for _ in 0..5 {
        app.press(row).unwrap();
        if app
            .wait_for_within(pane, std::time::Duration::from_secs(1))
            .is_ok()
        {
            return;
        }
    }
    app.wait_for(pane).unwrap();
}

pub fn enable_demo_mode_in_db(db_path: &std::path::Path) {
    let db = factory_core::ProjectDatabase::open(db_path).unwrap();
    let mut prefs = db
        .load_preferences()
        .unwrap()
        .unwrap_or_else(factory_core::AppPreferences::empty);
    prefs.demo_mode = true;
    db.save_preferences(&prefs).unwrap();
}

pub fn start_with_demo_db(db_path: PathBuf) -> Headless {
    enable_demo_mode_in_db(&db_path);
    let mut config = EngineConfig::new(factory_app::root_with_db_path(db_path));
    config.environment.os = Os::MacOs;
    config.viewport = Size::new(px(1360.0), px(920.0));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("Issues resolved").unwrap();
    app
}

#[cfg(unix)]
pub fn write_fake_claude(dir: &std::path::Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

pub fn isolated_home_environment(temp: &std::path::Path) -> factory_core::HomeEnvironment {
    factory_core::HomeEnvironment {
        env_override: None,
        default_root: temp.join("home").join("Bruk"),
        locator_path: temp.join("support").join("Bruk").join("location"),
        legacy_database_paths: vec![
            temp.join("support")
                .join("software-factory")
                .join("factory.db"),
        ],
    }
}

pub fn start_with_home_environment(environment: factory_core::HomeEnvironment) -> Headless {
    let mut config = EngineConfig::new(factory_app::root_with_home_environment(
        environment,
        OsString::new(),
    ));
    config.environment.os = Os::MacOs;
    config.viewport = Size::new(px(1360.0), px(920.0));
    Headless::start(config).unwrap()
}

/// Creates a git repository with one commit on `trunk` to clone from.
pub fn init_origin_repo(origin: &std::path::Path) -> PathBuf {
    use std::process::{Command, Stdio};

    std::fs::create_dir_all(origin).unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(origin)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "git {args:?} failed"
        );
    };
    git(&["init", "-b", "trunk"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(origin.join("README.md"), "# billing-api\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "init"]);
    origin.to_path_buf()
}
