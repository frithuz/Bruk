//! The Bruk folder: one user-visible directory holding Bruk's database, configuration,
//! Bruk-managed project clones, and run workspaces.
//!
//! ```text
//! ~/Bruk/
//! ├── bruk.db           SQLite: projects, preferences, project settings, secrets, run history
//! ├── projects/<name>/  Repositories Bruk cloned for you
//! ├── runs/<run-id>/    Per-run step bundles and isolated git worktrees
//! └── cache/modules/    Git modules fetched for `.agents/bruk/bruk.yaml`
//! ```
//!
//! External projects (clones that already exist elsewhere on disk) stay where they are;
//! Bruk only records their path in `bruk.db`.
//!
//! The folder location resolves in this order:
//! 1. The `BRUK_HOME` environment variable.
//! 2. The location remembered from first-launch setup, stored as a single line in
//!    `<platform config dir>/Bruk/location`. This file is only written when the user picks
//!    a folder other than `~/Bruk`.
//! 3. `~/Bruk`, once it contains `bruk.db`.
//!
//! When none of these applies, Bruk shows first-launch setup.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use thiserror::Error;

use crate::{claude_cli::expand_tilde, storage::ProjectDatabase};

/// Environment variable that points Bruk at an explicit folder, skipping first-launch setup.
pub const BRUK_HOME_ENV: &str = "BRUK_HOME";
/// Folder name created inside the user's home directory by default (`~/Bruk`).
pub const DEFAULT_HOME_DIR_NAME: &str = "Bruk";
/// SQLite database file inside the Bruk folder.
pub const HOME_DATABASE_FILE: &str = "bruk.db";
/// Directory holding repositories that Bruk cloned.
pub const HOME_PROJECTS_DIR: &str = "projects";
/// Directory holding per-run step bundles and worktrees.
pub const HOME_RUNS_DIR: &str = "runs";
/// Per-chat runtime files (agent instructions, Codex's isolated home) inside the Bruk folder.
pub const HOME_CHATS_DIR: &str = "chats";
/// Directory holding fetched git modules.
pub const HOME_MODULE_CACHE_DIR: &str = "cache/modules";

const LOCATOR_APP_DIR: &str = "Bruk";
const LOCATOR_FILE: &str = "location";
const LEGACY_APP_DIR: &str = "software-factory";
const LEGACY_DATABASE_FILE: &str = "factory.db";

/// Errors raised while resolving or preparing the Bruk folder.
#[derive(Debug, Error)]
pub enum HomeError {
    #[error("could not determine your home folder")]
    HomeDirUnavailable,
    #[error("could not determine the application support folder")]
    ConfigDirUnavailable,
    #[error("choose a folder for Bruk's data")]
    EmptyPath,
    #[error("'{0}' is not an absolute path")]
    RelativePath(PathBuf),
    #[error("'{0}' is a file, not a folder")]
    NotADirectory(PathBuf),
    #[error("failed to create '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to remember the Bruk folder location in '{path}': {source}")]
    Locator {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to import data from '{path}': {message}")]
    Import { path: PathBuf, message: String },
}

/// A resolved Bruk folder and the paths Bruk uses inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrukHome {
    root: PathBuf,
    database_path: PathBuf,
}

impl BrukHome {
    /// A Bruk folder rooted at `root` using the standard `bruk.db` database file.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let database_path = root.join(HOME_DATABASE_FILE);
        Self {
            root,
            database_path,
        }
    }

    /// A Bruk folder whose root is the parent directory of an explicit database file.
    ///
    /// Used by tests and tools that open a specific SQLite file.
    pub fn with_database_path(database_path: impl Into<PathBuf>) -> Self {
        let database_path = database_path.into();
        let root = database_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            root,
            database_path,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.root.join(HOME_PROJECTS_DIR)
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.root.join(HOME_RUNS_DIR)
    }

    /// Runtime directory for one workflow run (step bundles and its isolated worktree).
    pub fn run_dir(&self, run_id: &str) -> PathBuf {
        self.runs_dir().join(run_id)
    }

    pub fn chats_dir(&self) -> PathBuf {
        self.root.join(HOME_CHATS_DIR)
    }

    /// Runtime directory for one chat session, kept across its turns.
    pub fn chat_dir(&self, chat_id: &str) -> PathBuf {
        self.chats_dir().join(chat_id)
    }

    pub fn module_cache_dir(&self) -> PathBuf {
        self.root.join(HOME_MODULE_CACHE_DIR)
    }

    /// Where Bruk clones a project named `project_name`.
    pub fn managed_project_dir(&self, project_name: &str) -> PathBuf {
        self.projects_dir().join(project_dir_name(project_name))
    }

    /// Whether `path` is a clone that lives in this folder's `projects/` directory.
    pub fn is_managed_project(&self, path: &Path) -> bool {
        path.starts_with(self.projects_dir())
    }

    /// A folder counts as set up once its database exists.
    pub fn is_initialized(&self) -> bool {
        self.database_path.is_file()
    }

    /// Creates the folder and its standard subdirectories if they are missing.
    pub fn ensure_layout(&self) -> Result<(), HomeError> {
        for dir in [
            self.root.clone(),
            self.projects_dir(),
            self.runs_dir(),
            self.chats_dir(),
            self.module_cache_dir(),
        ] {
            fs::create_dir_all(&dir)
                .map_err(|source| HomeError::CreateDir { path: dir, source })?;
        }
        Ok(())
    }
}

/// Outcome of resolving the Bruk folder at launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeResolution {
    /// A folder is configured; open it.
    Ready(BrukHome),
    /// No folder is set up yet; ask the user, starting from `suggested_root`.
    NeedsSetup { suggested_root: PathBuf },
}

/// Result of completing first-launch setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeSetup {
    pub home: BrukHome,
    /// Legacy database whose contents were copied into the new folder, if any.
    pub imported_from: Option<PathBuf>,
    /// The chosen folder already contained a Bruk database and was adopted as-is.
    pub adopted_existing: bool,
}

/// Inputs used to locate the Bruk folder. [`HomeEnvironment::system`] reads the real machine;
/// tests build one by hand so they never touch the user's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeEnvironment {
    /// Explicit folder from `BRUK_HOME`.
    pub env_override: Option<PathBuf>,
    /// Folder offered by default (`~/Bruk`).
    pub default_root: PathBuf,
    /// File that remembers a non-default folder chosen during setup.
    pub locator_path: PathBuf,
    /// Pre-`~/Bruk` database locations, checked in order, offered for one-time import.
    pub legacy_database_paths: Vec<PathBuf>,
}

impl HomeEnvironment {
    /// Reads the environment, home directory, and platform config directory of this machine.
    pub fn system() -> Result<Self, HomeError> {
        let home_dir = dirs::home_dir().ok_or(HomeError::HomeDirUnavailable)?;
        let config_dir = dirs::config_dir().ok_or(HomeError::ConfigDirUnavailable)?;

        let env_override = env::var(BRUK_HOME_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| expand_tilde(&value));

        let mut legacy_database_paths = Vec::new();
        if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
            && !xdg.trim().is_empty()
        {
            legacy_database_paths.push(
                PathBuf::from(xdg.trim())
                    .join(LEGACY_APP_DIR)
                    .join(LEGACY_DATABASE_FILE),
            );
        }
        legacy_database_paths.push(config_dir.join(LEGACY_APP_DIR).join(LEGACY_DATABASE_FILE));

        Ok(Self {
            env_override,
            default_root: home_dir.join(DEFAULT_HOME_DIR_NAME),
            locator_path: config_dir.join(LOCATOR_APP_DIR).join(LOCATOR_FILE),
            legacy_database_paths,
        })
    }

    /// The folder remembered from a previous setup, if the locator file names one.
    pub fn remembered_root(&self) -> Option<PathBuf> {
        let raw = fs::read_to_string(&self.locator_path).ok()?;
        let line = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
        let path = PathBuf::from(line);
        path.is_absolute().then_some(path)
    }

    /// Decides which folder to open, or that first-launch setup is needed.
    pub fn resolve(&self) -> HomeResolution {
        if let Some(root) = &self.env_override {
            return HomeResolution::Ready(BrukHome::at(root.clone()));
        }
        if let Some(root) = self.remembered_root() {
            let home = BrukHome::at(root);
            return if home.is_initialized() {
                HomeResolution::Ready(home)
            } else {
                HomeResolution::NeedsSetup {
                    suggested_root: home.root().to_path_buf(),
                }
            };
        }
        let default_home = BrukHome::at(self.default_root.clone());
        if default_home.is_initialized() {
            HomeResolution::Ready(default_home)
        } else {
            HomeResolution::NeedsSetup {
                suggested_root: self.default_root.clone(),
            }
        }
    }

    /// The first legacy database that exists on disk.
    pub fn legacy_database(&self) -> Option<PathBuf> {
        self.legacy_database_paths
            .iter()
            .find(|path| path.is_file())
            .cloned()
    }

    /// Completes first-launch setup for the folder typed or picked by the user.
    ///
    /// Creates the folder layout, optionally copies the legacy database in (only when the
    /// folder has no database yet), and remembers the location for future launches. The
    /// legacy database is left untouched.
    pub fn set_up(&self, root_input: &str, import_legacy: bool) -> Result<HomeSetup, HomeError> {
        let trimmed = root_input.trim();
        if trimmed.is_empty() {
            return Err(HomeError::EmptyPath);
        }
        let root = expand_tilde(trimmed);
        if !root.is_absolute() {
            return Err(HomeError::RelativePath(root));
        }
        if root.exists() && !root.is_dir() {
            return Err(HomeError::NotADirectory(root));
        }

        let home = BrukHome::at(root.clone());
        let adopted_existing = home.is_initialized();
        home.ensure_layout()?;

        let mut imported_from = None;
        if import_legacy
            && !adopted_existing
            && let Some(legacy) = self.legacy_database()
        {
            ProjectDatabase::copy_database(&legacy, home.database_path()).map_err(|err| {
                HomeError::Import {
                    path: legacy.clone(),
                    message: err.to_string(),
                }
            })?;
            imported_from = Some(legacy);
        }

        self.remember_root(&root)?;
        Ok(HomeSetup {
            home,
            imported_from,
            adopted_existing,
        })
    }

    fn remember_root(&self, root: &Path) -> Result<(), HomeError> {
        let locator_error = |source| HomeError::Locator {
            path: self.locator_path.clone(),
            source,
        };
        if root == self.default_root {
            // The default folder is found without a locator; drop any stale one.
            return match fs::remove_file(&self.locator_path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(locator_error(err)),
            };
        }
        if let Some(parent) = self.locator_path.parent() {
            fs::create_dir_all(parent).map_err(locator_error)?;
        }
        fs::write(&self.locator_path, format!("{}\n", root.display())).map_err(locator_error)
    }
}

/// The Bruk folder configured on this machine, if setup has happened (or `BRUK_HOME` is set).
pub fn resolve_system_home() -> Option<BrukHome> {
    match HomeEnvironment::system().ok()?.resolve() {
        HomeResolution::Ready(home) => Some(home),
        HomeResolution::NeedsSetup { .. } => None,
    }
}

/// Shortens a path under the user's home directory to `~/...` for display.
pub fn display_with_tilde(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

/// Turns a project name into a safe directory name for `projects/`.
pub fn project_dir_name(project_name: &str) -> String {
    let mut out = String::new();
    for ch in project_name.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        "project".to_owned()
    } else {
        trimmed.to_owned()
    }
}

// --- Cloning projects into the Bruk folder -----------------------------------------------

/// Errors raised while cloning a repository into the Bruk folder.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CloneError {
    #[error("enter a repository URL or owner/repo")]
    EmptySource,
    #[error("'{0}' already exists. Link it as an existing clone or choose another project name.")]
    DestinationExists(PathBuf),
    #[error("failed to create '{path}': {message}")]
    CreateDir { path: PathBuf, message: String },
    #[error("failed to run git: {0}")]
    GitUnavailable(String),
    #[error("git clone failed: {0}")]
    CloneFailed(String),
}

/// A repository address parsed from what the user typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneSource {
    /// URL or path handed to `git clone`.
    pub url: String,
    /// Project name derived from the repository name.
    pub suggested_name: String,
}

/// Parses a git URL, scp-style address, local path, or GitHub `owner/repo` shorthand.
pub fn parse_clone_source(input: &str) -> Result<CloneSource, CloneError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CloneError::EmptySource);
    }
    let url = if is_github_shorthand(trimmed) {
        let slug = trimmed.strip_suffix(".git").unwrap_or(trimmed);
        format!("https://github.com/{slug}.git")
    } else {
        trimmed.to_owned()
    };
    let suggested_name = repository_name(&url);
    Ok(CloneSource {
        url,
        suggested_name,
    })
}

fn is_github_shorthand(input: &str) -> bool {
    let mut parts = input.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let valid = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    valid(owner) && valid(repo)
}

fn repository_name(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', ':', '\\']).next().unwrap_or(trimmed);
    let name = last.strip_suffix(".git").unwrap_or(last);
    project_dir_name(name)
}

/// A repository cloned into the Bruk folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClonedRepository {
    pub path: PathBuf,
    /// Branch checked out after cloning (the remote's default branch).
    pub default_branch: String,
}

/// Clones `url` into `destination`, which must not exist or be an empty directory.
///
/// Git never prompts for credentials here (`GIT_TERMINAL_PROMPT=0`), so a repository that
/// needs them fails fast with git's error instead of hanging the app.
pub fn clone_repository(url: &str, destination: &Path) -> Result<ClonedRepository, CloneError> {
    if destination.exists() && !is_empty_dir(destination) {
        return Err(CloneError::DestinationExists(destination.to_path_buf()));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|err| CloneError::CreateDir {
            path: parent.to_path_buf(),
            message: err.to_string(),
        })?;
    }

    let output = Command::new("git")
        .args(["clone", "--quiet", "--", url])
        .arg(destination)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|err| CloneError::GitUnavailable(err.to_string()))?;
    if !output.status.success() {
        if is_empty_dir(destination) {
            let _ = fs::remove_dir(destination);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(CloneError::CloneFailed(if stderr.is_empty() {
            output.status.to_string()
        } else {
            stderr
        }));
    }

    let default_branch = current_branch(destination).unwrap_or_else(|| "main".to_owned());
    Ok(ClonedRepository {
        path: destination.to_path_buf(),
        default_branch,
    })
}

fn current_branch(repo: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(repo)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !branch.is_empty()).then_some(branch)
}

fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

// --- Creating new local repositories ----------------------------------------------------

/// Errors raised while creating and initializing a new local git repository.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InitRepositoryError {
    #[error("enter a project name")]
    EmptyProjectName,
    #[error("'{0}' is not a valid branch name")]
    InvalidBranchName(String),
    #[error("'{0}' is not an absolute path")]
    RelativePath(PathBuf),
    #[error("'{0}' is a file, not a folder")]
    NotADirectory(PathBuf),
    #[error("'{0}' already exists. Link it as an existing clone or choose another project name.")]
    DestinationExists(PathBuf),
    #[error("failed to create '{path}': {message}")]
    CreateDir { path: PathBuf, message: String },
    #[error("failed to write '{path}': {message}")]
    WriteFile { path: PathBuf, message: String },
    #[error("failed to run git: {0}")]
    GitUnavailable(String),
    #[error("git init failed: {0}")]
    InitFailed(String),
    #[error("initial commit failed: {0}")]
    InitialCommitFailed(String),
}

/// Options for initializing a new local git repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRepositoryOptions {
    /// Initial branch name (defaults to `"main"` when blank).
    pub default_branch: String,
    /// Create a starter `README.md` with `# <project_name>`.
    pub create_readme: bool,
    /// Project title used for the starter `README.md`.
    pub project_name: String,
    /// Create an initial commit so `HEAD` exists right away (required for git worktrees).
    pub initial_commit: bool,
}

impl Default for InitRepositoryOptions {
    fn default() -> Self {
        Self {
            default_branch: "main".to_owned(),
            create_readme: true,
            project_name: String::new(),
            initial_commit: true,
        }
    }
}

/// A newly created and initialized local git repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializedRepository {
    pub path: PathBuf,
    /// Initial branch checked out in the new repository.
    pub default_branch: String,
}

/// Validates and normalizes an initial git branch name (blank defaults to `"main"`).
pub fn validate_branch_name(raw: &str) -> Result<String, InitRepositoryError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok("main".to_owned());
    }
    let invalid = trimmed == "@"
        || trimmed == "HEAD"
        || trimmed.starts_with('-')
        || trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.ends_with('.')
        || trimmed.contains("..")
        || trimmed.contains("//")
        || trimmed.contains("@{")
        || trimmed.chars().any(|ch| {
            ch.is_control()
                || ch.is_whitespace()
                || matches!(ch, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        || trimmed
            .split('/')
            .any(|seg| seg.is_empty() || seg.starts_with('.') || seg.ends_with(".lock"));
    if invalid {
        return Err(InitRepositoryError::InvalidBranchName(trimmed.to_owned()));
    }
    Ok(trimmed.to_owned())
}

/// Resolves the directory where a new local repository will be created.
///
/// When `parent_input` is blank, the repository lands inside `default_projects_dir`.
pub fn resolve_new_repository_destination(
    parent_input: &str,
    default_projects_dir: &Path,
    project_name: &str,
) -> Result<PathBuf, InitRepositoryError> {
    let name = project_name.trim();
    if name.is_empty() {
        return Err(InitRepositoryError::EmptyProjectName);
    }
    let parent = match parent_input.trim() {
        "" => default_projects_dir.to_path_buf(),
        custom => {
            let expanded = expand_tilde(custom);
            if !expanded.is_absolute() {
                return Err(InitRepositoryError::RelativePath(expanded));
            }
            if expanded.exists() && !expanded.is_dir() {
                return Err(InitRepositoryError::NotADirectory(expanded));
            }
            expanded
        }
    };
    Ok(parent.join(project_dir_name(name)))
}

/// Creates `destination`, initializes a local git repository there, and optionally creates a
/// starter `README.md` and initial commit so git worktrees work immediately.
pub fn init_local_repository(
    destination: &Path,
    options: &InitRepositoryOptions,
) -> Result<InitializedRepository, InitRepositoryError> {
    if !destination.is_absolute() {
        return Err(InitRepositoryError::RelativePath(destination.to_path_buf()));
    }
    if destination.exists() {
        if !destination.is_dir() {
            return Err(InitRepositoryError::NotADirectory(
                destination.to_path_buf(),
            ));
        }
        if !is_empty_dir(destination) {
            return Err(InitRepositoryError::DestinationExists(
                destination.to_path_buf(),
            ));
        }
    }

    let branch = validate_branch_name(&options.default_branch)?;
    let existed = destination.exists();
    fs::create_dir_all(destination).map_err(|err| InitRepositoryError::CreateDir {
        path: destination.to_path_buf(),
        message: err.to_string(),
    })?;

    let cleanup = || {
        if !existed {
            let _ = fs::remove_dir_all(destination);
        } else {
            let _ = fs::remove_dir_all(destination.join(".git"));
            let _ = fs::remove_file(destination.join("README.md"));
        }
    };

    let init_output = Command::new("git")
        .args(["init", "--quiet", "-b", &branch])
        .current_dir(destination)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|err| {
            cleanup();
            InitRepositoryError::GitUnavailable(err.to_string())
        })?;
    if !init_output.status.success() {
        cleanup();
        let stderr = String::from_utf8_lossy(&init_output.stderr)
            .trim()
            .to_owned();
        return Err(InitRepositoryError::InitFailed(if stderr.is_empty() {
            init_output.status.to_string()
        } else {
            stderr
        }));
    }

    if options.create_readme {
        let title = match options.project_name.trim() {
            "" => destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Project"),
            name => name,
        };
        let readme_path = destination.join("README.md");
        if let Err(err) = fs::write(&readme_path, format!("# {title}\n")) {
            cleanup();
            return Err(InitRepositoryError::WriteFile {
                path: readme_path,
                message: err.to_string(),
            });
        }
    }

    if options.initial_commit {
        if options.create_readme {
            let add_output = Command::new("git")
                .args(["add", "--", "README.md"])
                .current_dir(destination)
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdin(Stdio::null())
                .output()
                .map_err(|err| {
                    cleanup();
                    InitRepositoryError::GitUnavailable(err.to_string())
                })?;
            if !add_output.status.success() {
                cleanup();
                let stderr = String::from_utf8_lossy(&add_output.stderr)
                    .trim()
                    .to_owned();
                return Err(InitRepositoryError::InitialCommitFailed(
                    if stderr.is_empty() {
                        add_output.status.to_string()
                    } else {
                        stderr
                    },
                ));
            }
        }

        let mut commit_cmd = Command::new("git");
        commit_cmd
            .current_dir(destination)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .args(["-c", "commit.gpgsign=false"]);
        if git_config_value(destination, "user.name").is_none() {
            commit_cmd.args(["-c", "user.name=Bruk"]);
        }
        if git_config_value(destination, "user.email").is_none() {
            commit_cmd.args(["-c", "user.email=bruk@localhost"]);
        }
        commit_cmd.args(["commit", "--quiet"]);
        if !options.create_readme {
            commit_cmd.arg("--allow-empty");
        }
        commit_cmd.args(["-m", "Initial commit"]);

        let commit_output = commit_cmd.output().map_err(|err| {
            cleanup();
            InitRepositoryError::GitUnavailable(err.to_string())
        })?;
        if !commit_output.status.success() {
            cleanup();
            let stderr = String::from_utf8_lossy(&commit_output.stderr)
                .trim()
                .to_owned();
            return Err(InitRepositoryError::InitialCommitFailed(
                if stderr.is_empty() {
                    commit_output.status.to_string()
                } else {
                    stderr
                },
            ));
        }
    }

    let default_branch = current_branch(destination).unwrap_or(branch);
    Ok(InitializedRepository {
        path: destination.to_path_buf(),
        default_branch,
    })
}

fn git_config_value(repo: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", key])
        .current_dir(repo)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn test_environment(temp: &Path) -> HomeEnvironment {
        HomeEnvironment {
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

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn fresh_machine_needs_setup_at_default_folder() {
        let temp = TempDir::new().unwrap();
        let env = test_environment(temp.path());
        assert_eq!(
            env.resolve(),
            HomeResolution::NeedsSetup {
                suggested_root: env.default_root.clone()
            }
        );
        assert_eq!(env.legacy_database(), None);
    }

    #[test]
    fn env_override_wins_without_setup() {
        let temp = TempDir::new().unwrap();
        let mut env = test_environment(temp.path());
        env.env_override = Some(temp.path().join("from-env"));
        assert_eq!(
            env.resolve(),
            HomeResolution::Ready(BrukHome::at(temp.path().join("from-env")))
        );
    }

    #[test]
    fn default_folder_setup_writes_no_locator_and_resolves_afterwards() {
        let temp = TempDir::new().unwrap();
        let env = test_environment(temp.path());

        let setup = env
            .set_up(&env.default_root.display().to_string(), true)
            .unwrap();
        assert_eq!(setup.imported_from, None);
        assert!(!setup.adopted_existing);
        for dir in ["projects", "runs", "chats", "cache/modules"] {
            assert!(env.default_root.join(dir).is_dir(), "{dir} missing");
        }
        assert!(!env.locator_path.exists());

        ProjectDatabase::open_home(&setup.home).unwrap();
        assert_eq!(
            env.resolve(),
            HomeResolution::Ready(BrukHome::at(env.default_root.clone()))
        );
    }

    #[test]
    fn custom_folder_is_remembered_and_imports_legacy_database_once() {
        let temp = TempDir::new().unwrap();
        let env = test_environment(temp.path());
        let legacy_path = env.legacy_database_paths[0].clone();
        {
            let legacy = ProjectDatabase::open(&legacy_path).unwrap();
            legacy
                .register_project("billing-api", "/Users/dev/billing-api", "main", "")
                .unwrap();
        }
        assert_eq!(env.legacy_database(), Some(legacy_path.clone()));

        let custom = temp.path().join("Data").join("Bruk");
        let setup = env.set_up(&custom.display().to_string(), true).unwrap();
        assert_eq!(setup.imported_from, Some(legacy_path.clone()));
        assert_eq!(env.remembered_root(), Some(custom.clone()));
        assert_eq!(
            env.resolve(),
            HomeResolution::Ready(BrukHome::at(custom.clone()))
        );

        let db = ProjectDatabase::open_home(&setup.home).unwrap();
        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "billing-api");
        // The legacy database is left in place.
        assert!(legacy_path.is_file());

        // Choosing the same folder again adopts it instead of re-importing.
        let again = env.set_up(&custom.display().to_string(), true).unwrap();
        assert!(again.adopted_existing);
        assert_eq!(again.imported_from, None);

        // Switching back to the default folder clears the locator.
        env.set_up(&env.default_root.display().to_string(), false)
            .unwrap();
        assert!(!env.locator_path.exists());
    }

    #[test]
    fn remembered_folder_without_database_asks_for_setup_there() {
        let temp = TempDir::new().unwrap();
        let env = test_environment(temp.path());
        let custom = temp.path().join("Gone");
        fs::create_dir_all(env.locator_path.parent().unwrap()).unwrap();
        fs::write(&env.locator_path, format!("{}\n", custom.display())).unwrap();
        assert_eq!(
            env.resolve(),
            HomeResolution::NeedsSetup {
                suggested_root: custom
            }
        );
    }

    #[test]
    fn rejects_empty_relative_and_file_paths() {
        let temp = TempDir::new().unwrap();
        let env = test_environment(temp.path());
        assert!(matches!(env.set_up("  ", false), Err(HomeError::EmptyPath)));
        assert!(matches!(
            env.set_up("relative/Bruk", false),
            Err(HomeError::RelativePath(_))
        ));
        let file_path = temp.path().join("file.txt");
        fs::write(&file_path, "x").unwrap();
        assert!(matches!(
            env.set_up(&file_path.display().to_string(), false),
            Err(HomeError::NotADirectory(_))
        ));
    }

    #[test]
    fn home_paths_and_managed_projects() {
        let home = BrukHome::at("/Users/dev/Bruk");
        assert_eq!(home.database_path(), Path::new("/Users/dev/Bruk/bruk.db"));
        assert_eq!(
            home.run_dir("run-7"),
            Path::new("/Users/dev/Bruk/runs/run-7")
        );
        assert_eq!(
            home.module_cache_dir(),
            Path::new("/Users/dev/Bruk/cache/modules")
        );
        assert_eq!(
            home.managed_project_dir("Billing API"),
            Path::new("/Users/dev/Bruk/projects/Billing-API")
        );
        assert!(home.is_managed_project(Path::new("/Users/dev/Bruk/projects/Billing-API")));
        assert!(!home.is_managed_project(Path::new("/Users/dev/code/billing-api")));

        let from_db = BrukHome::with_database_path("/tmp/test/factory.db");
        assert_eq!(from_db.root(), Path::new("/tmp/test"));
        assert_eq!(from_db.runs_dir(), Path::new("/tmp/test/runs"));
        assert_eq!(
            from_db.chat_dir("chat-1"),
            Path::new("/tmp/test/chats/chat-1")
        );
    }

    #[test]
    fn project_dir_names_are_filesystem_safe() {
        assert_eq!(project_dir_name("billing-api"), "billing-api");
        assert_eq!(project_dir_name("  My Service / v2 "), "My-Service-v2");
        assert_eq!(project_dir_name("../../etc"), "etc");
        assert_eq!(project_dir_name("???"), "project");
    }

    #[test]
    fn parses_clone_sources() {
        assert_eq!(
            parse_clone_source("acme/billing-api").unwrap(),
            CloneSource {
                url: "https://github.com/acme/billing-api.git".to_owned(),
                suggested_name: "billing-api".to_owned(),
            }
        );
        assert_eq!(
            parse_clone_source("acme/billing-api.git").unwrap().url,
            "https://github.com/acme/billing-api.git"
        );
        assert_eq!(
            parse_clone_source("git@github.com:acme/payments.git").unwrap(),
            CloneSource {
                url: "git@github.com:acme/payments.git".to_owned(),
                suggested_name: "payments".to_owned(),
            }
        );
        assert_eq!(
            parse_clone_source("https://gitlab.com/group/sub/tool/")
                .unwrap()
                .suggested_name,
            "tool"
        );
        assert_eq!(
            parse_clone_source("/Users/dev/src/local-repo").unwrap(),
            CloneSource {
                url: "/Users/dev/src/local-repo".to_owned(),
                suggested_name: "local-repo".to_owned(),
            }
        );
        assert_eq!(parse_clone_source("   "), Err(CloneError::EmptySource));
    }

    #[test]
    fn clones_local_repository_and_detects_default_branch() {
        let temp = TempDir::new().unwrap();
        let origin = temp.path().join("origin");
        fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-b", "trunk"]);
        git(&origin, &["config", "user.email", "test@example.com"]);
        git(&origin, &["config", "user.name", "Test"]);
        fs::write(origin.join("README.md"), "hello\n").unwrap();
        git(&origin, &["add", "."]);
        git(&origin, &["commit", "-m", "init"]);

        let home = BrukHome::at(temp.path().join("Bruk"));
        let destination = home.managed_project_dir("origin");
        let cloned = clone_repository(&origin.display().to_string(), &destination).unwrap();
        assert_eq!(cloned.path, destination);
        assert_eq!(cloned.default_branch, "trunk");
        assert!(destination.join("README.md").is_file());

        // A second clone into the same folder is refused.
        assert_eq!(
            clone_repository(&origin.display().to_string(), &destination),
            Err(CloneError::DestinationExists(destination.clone()))
        );

        // A missing repository reports git's error and leaves no folder behind.
        let missing = home.managed_project_dir("missing");
        let err = clone_repository(&temp.path().join("nope").display().to_string(), &missing)
            .unwrap_err();
        assert!(matches!(err, CloneError::CloneFailed(_)), "{err:?}");
        assert!(!missing.exists());
    }

    #[test]
    fn validates_branch_names_for_new_repositories() {
        assert_eq!(validate_branch_name("").unwrap(), "main");
        assert_eq!(validate_branch_name("   ").unwrap(), "main");
        assert_eq!(validate_branch_name("trunk").unwrap(), "trunk");
        assert_eq!(validate_branch_name("release/v1").unwrap(), "release/v1");

        for bad in [
            "-b",
            "my branch",
            "a..b",
            "foo.lock",
            "@",
            "HEAD",
            "a//b",
            "a/.hidden",
            "feat~1",
            "trailing/",
            "trailing.",
        ] {
            assert!(
                matches!(
                    validate_branch_name(bad),
                    Err(InitRepositoryError::InvalidBranchName(_))
                ),
                "expected '{bad}' to be rejected"
            );
        }
    }

    #[test]
    fn resolves_new_repository_destinations() {
        let temp = TempDir::new().unwrap();
        let default_dir = temp.path().join("Bruk").join("projects");
        let custom_dir = temp.path().join("Code");
        fs::create_dir_all(&custom_dir).unwrap();
        let file_path = temp.path().join("not-a-dir");
        fs::write(&file_path, "x").unwrap();

        assert_eq!(
            resolve_new_repository_destination("", &default_dir, "My Service").unwrap(),
            default_dir.join("My-Service")
        );
        assert_eq!(
            resolve_new_repository_destination(
                &custom_dir.display().to_string(),
                &default_dir,
                "My Service",
            )
            .unwrap(),
            custom_dir.join("My-Service")
        );
        assert_eq!(
            resolve_new_repository_destination("", &default_dir, "   "),
            Err(InitRepositoryError::EmptyProjectName)
        );
        assert!(matches!(
            resolve_new_repository_destination("relative/path", &default_dir, "app"),
            Err(InitRepositoryError::RelativePath(_))
        ));
        assert!(matches!(
            resolve_new_repository_destination(
                &file_path.display().to_string(),
                &default_dir,
                "app",
            ),
            Err(InitRepositoryError::NotADirectory(_))
        ));
    }

    #[test]
    fn initializes_local_repository_with_and_without_readme() {
        let temp = TempDir::new().unwrap();
        let home = BrukHome::at(temp.path().join("Bruk"));

        // Default options with custom branch and starter README.
        let dest = home.managed_project_dir("Local Tool");
        let initialized = init_local_repository(
            &dest,
            &InitRepositoryOptions {
                default_branch: "trunk".to_owned(),
                create_readme: true,
                project_name: "Local Tool".to_owned(),
                initial_commit: true,
            },
        )
        .unwrap();
        assert_eq!(initialized.path, dest);
        assert_eq!(initialized.default_branch, "trunk");
        assert!(dest.join(".git").is_dir());
        assert_eq!(
            fs::read_to_string(dest.join("README.md")).unwrap(),
            "# Local Tool\n"
        );
        // HEAD resolves immediately so worktrees work out of the box.
        git(&dest, &["rev-parse", "--verify", "HEAD"]);
        let status_out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&dest)
            .output()
            .unwrap();
        assert!(status_out.status.success());
        assert!(
            String::from_utf8_lossy(&status_out.stdout)
                .trim()
                .is_empty()
        );

        // Creating again into the occupied directory is refused.
        assert_eq!(
            init_local_repository(&dest, &InitRepositoryOptions::default()),
            Err(InitRepositoryError::DestinationExists(dest.clone()))
        );

        // Creating without README still creates an empty initial commit with HEAD.
        let bare_dest = temp.path().join("existing-empty");
        fs::create_dir_all(&bare_dest).unwrap();
        let initialized_empty = init_local_repository(
            &bare_dest,
            &InitRepositoryOptions {
                default_branch: String::new(),
                create_readme: false,
                project_name: "No Readme".to_owned(),
                initial_commit: true,
            },
        )
        .unwrap();
        assert_eq!(initialized_empty.default_branch, "main");
        assert!(bare_dest.join(".git").is_dir());
        assert!(!bare_dest.join("README.md").exists());
        git(&bare_dest, &["rev-parse", "--verify", "HEAD"]);

        // Invalid branch leaves no directory behind.
        let bad_dest = home.managed_project_dir("bad-branch-repo");
        assert!(matches!(
            init_local_repository(
                &bad_dest,
                &InitRepositoryOptions {
                    default_branch: "bad branch".to_owned(),
                    ..InitRepositoryOptions::default()
                },
            ),
            Err(InitRepositoryError::InvalidBranchName(_))
        ));
        assert!(!bad_dest.exists());

        // Relative destination and file destination are rejected.
        assert!(matches!(
            init_local_repository(
                Path::new("relative/repo"),
                &InitRepositoryOptions::default()
            ),
            Err(InitRepositoryError::RelativePath(_))
        ));
        let file_dest = temp.path().join("plain-file");
        fs::write(&file_dest, "hi").unwrap();
        assert!(matches!(
            init_local_repository(&file_dest, &InitRepositoryOptions::default()),
            Err(InitRepositoryError::NotADirectory(_))
        ));
    }
}
