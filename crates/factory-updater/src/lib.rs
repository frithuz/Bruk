//! Native Shell (`nsh-updater`) self-update engine for packaged macOS builds.
//!
//! `nsh package --target macos` writes `Contents/Resources/nsh/assets/nsh-updater.json`
//! from `nsh.json`'s `"updater"` section. At runtime this module:
//!
//! 1. Reads that shipped configuration and checks `releases/latest/download/latest.json`
//!    (plus `latest.json.sig` when `publicKey` is configured) via `ureq` with the OS
//!    certificate verifier (`RootCerts::PlatformVerifier`).
//! 2. Streams the verified `.tar.gz` update archive to a cache file while checking size
//!    and SHA-256 incrementally with `nsh_updater::Download`.
//! 3. Atomically replaces `Bruk.app` via `nsh_updater::install::Installation` (either
//!    immediately before `nsh_core::element::desktop::restart()`, or staged to swap cleanly
//!    when the app quits).
//!
//! Development runs (`cargo run`, `nsh run`, and headless UI tests) have no packaged `.app`
//! bundle with `nsh-updater.json`, so update checks cleanly report `Unavailable` without
//! touching disk or mutating anything.

use std::{
    fs::{self, OpenOptions},
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use nsh_updater::{
    Available, CONFIG_ASSET, Config, Download,
    install::{Installation, Staged},
};

/// Maximum manifest size accepted over HTTP (1 MiB, matching `nsh-core`).
const MAX_MANIFEST_BYTES: usize = 1 << 20;
/// Maximum detached signature size accepted over HTTP (64 KiB).
const MAX_SIGNATURE_BYTES: usize = 64 * 1024;
/// Maximum number of HTTP redirects followed for manifest or asset downloads.
const MAX_REDIRECTS: usize = 10;
/// Streaming read buffer size (64 KiB).
const DOWNLOAD_CHUNK_BYTES: usize = 64 * 1024;
/// Minimum interval between UI progress callbacks during download.
const PROGRESS_THROTTLE: Duration = Duration::from_millis(80);

/// Metadata for an available newer release on GitHub Releases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: Option<String>,
    pub url: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub can_install: bool,
    pub install_blocked_reason: Option<String>,
}

/// Reactive UI state of the application auto-updater.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdaterState {
    #[default]
    Idle,
    Checking,
    UpToDate {
        current_version: String,
    },
    Available(AvailableUpdate),
    Downloading {
        update: AvailableUpdate,
        received: u64,
        total: Option<u64>,
    },
    ReadyToRestart {
        version: String,
        notes: Option<String>,
        archive_path: PathBuf,
    },
    Installing {
        version: String,
    },
    StagedOnQuit {
        version: String,
    },
    Unavailable(String),
    Error(String),
}

impl UpdaterState {
    /// Whether a background check, download, or install is currently running.
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing { .. }
        )
    }

    /// Download progress as a 0–100 percentage when total size is known.
    pub fn download_percent(&self) -> Option<u8> {
        let Self::Downloading {
            received,
            total: Some(total),
            ..
        } = self
        else {
            return None;
        };
        if *total == 0 {
            return None;
        }
        let pct = ((*received as f64 / *total as f64) * 100.0).clamp(0.0, 100.0);
        Some(pct.round() as u8)
    }
}

/// Outcome of a single update check against the configured release source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Running outside a packaged `.app` bundle (e.g. `cargo run` or tests).
    Unavailable(String),
    /// Running version is equal to or newer than the latest release.
    UpToDate { current_version: String },
    /// A newer version is published for this platform.
    Available(AvailableUpdate),
}

struct ShippedConfig {
    raw: Vec<u8>,
    config: Config,
    exe: PathBuf,
}

struct PendingOnQuit {
    version: String,
    staged: Staged,
    archive_path: PathBuf,
}

static STAGED_ON_QUIT: Mutex<Option<PendingOnQuit>> = Mutex::new(None);
static DOWNLOAD_SEQ: AtomicU64 = AtomicU64::new(0);

/// Returns the running Bruk version: the packaged `nsh-updater.json` version when running
/// inside a packaged `.app` bundle, or `CARGO_PKG_VERSION` in dev/test builds.
pub fn current_version() -> String {
    load_shipped_config()
        .ok()
        .flatten()
        .map(|shipped| shipped.config.version)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
}

/// Whether this process is running from a packaged `.app` bundle with an embedded
/// `nsh-updater.json` configuration.
pub fn has_packaged_updater_config() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| shipped_config_path_for_exe(&exe))
        .is_some_and(|path| path.is_file())
}

/// Resolves `<Name>.app/Contents/Resources/nsh/assets/nsh-updater.json` from a bundle executable.
pub fn shipped_config_path_for_exe(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    let contents_dir = macos_dir.parent()?;
    let bundle_dir = contents_dir.parent()?;
    if cfg!(target_os = "macos")
        && bundle_dir
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
    {
        return Some(
            contents_dir
                .join("Resources")
                .join("nsh")
                .join("assets")
                .join(CONFIG_ASSET),
        );
    }
    None
}

fn load_shipped_config() -> Result<Option<ShippedConfig>, String> {
    let Ok(exe) = std::env::current_exe() else {
        return Ok(None);
    };
    let Some(path) = shipped_config_path_for_exe(&exe) else {
        return Ok(None);
    };
    let raw = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "Could not read bundled updater configuration ({}): {err}",
                path.display()
            ));
        }
    };
    let config = Config::parse(&raw).map_err(|err| format!("Invalid updater config: {err}"))?;
    Ok(Some(ShippedConfig { raw, config, exe }))
}

/// Formats a byte count for display in the download UI (e.g. `14.2 MB`).
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Checks GitHub Releases (or any configured manifest source in `nsh-updater.json`) for a
/// newer version of Bruk. Blocking; call from a background thread.
pub fn check_for_update() -> Result<CheckOutcome, String> {
    let Some(shipped) = load_shipped_config()? else {
        return Ok(CheckOutcome::Unavailable(
            "Auto-updates are only available in packaged Bruk.app releases.".to_owned(),
        ));
    };

    let installation = Installation::of(&shipped.exe, &shipped.config.id, &shipped.raw);
    if let Some(inst) = &installation {
        inst.remove_leftovers();
    }

    let platform = nsh_updater::platform();
    check_with_config(
        &shipped.config,
        &platform,
        installation.as_ref(),
        &shipped.exe,
    )
}

fn check_with_config(
    config: &Config,
    platform: &str,
    installation: Option<&Installation>,
    exe: &Path,
) -> Result<CheckOutcome, String> {
    let manifest_url = config.manifest_url();
    let manifest_bytes = http_get_bytes(&manifest_url, MAX_MANIFEST_BYTES)
        .map_err(|err| format!("Could not check for updates: {err}"))?;

    let sig_bytes = match config.signature_url() {
        Some(sig_url) => Some(
            http_get_bytes(&sig_url, MAX_SIGNATURE_BYTES)
                .map_err(|err| format!("Could not download update signature: {err}"))?,
        ),
        None => None,
    };

    let available = config
        .check(&manifest_bytes, sig_bytes.as_deref(), platform)
        .map_err(|err| format!("Update check failed: {err}"))?;

    let Some(available) = available else {
        return Ok(CheckOutcome::UpToDate {
            current_version: config.version.clone(),
        });
    };

    Ok(CheckOutcome::Available(to_available_update(
        available,
        installation,
        exe,
    )))
}

fn to_available_update(
    available: Available,
    installation: Option<&Installation>,
    exe: &Path,
) -> AvailableUpdate {
    let translocated = exe.to_string_lossy().contains("/AppTranslocation/");
    let (can_install, install_blocked_reason) = if translocated {
        (
            false,
            Some("Move Bruk.app into /Applications to install updates automatically.".to_owned()),
        )
    } else if available.sha256.is_none() {
        (false, None)
    } else if let Some(inst) = installation {
        if inst.can_install() {
            (true, None)
        } else {
            (
                false,
                Some(
                    "Bruk cannot write to its installation folder. Download the update manually."
                        .to_owned(),
                ),
            )
        }
    } else {
        (false, None)
    };

    AvailableUpdate {
        version: available.version,
        notes: available.notes,
        url: available.url,
        sha256: available.sha256,
        size: available.size,
        can_install,
        install_blocked_reason,
    }
}

/// Downloads and verifies the release archive into Bruk's cache directory.
/// Blocking; call from a background thread.
pub fn download_update(
    update: &AvailableUpdate,
    on_progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let dir = updates_cache_dir()?;
    download_update_into(update, &dir, on_progress)
}

fn updates_cache_dir() -> Result<PathBuf, String> {
    let base = dirs::cache_dir()
        .or_else(|| Some(std::env::temp_dir()))
        .ok_or_else(|| "Could not resolve cache directory for update downloads.".to_owned())?;
    Ok(base.join("dev.frithuz.bruk").join("updates"))
}

fn download_update_into(
    update: &AvailableUpdate,
    dir: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let sha256 = update
        .sha256
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "This update does not include a verifiable archive.".to_owned())?;

    fs::create_dir_all(dir).map_err(|err| {
        format!(
            "Could not prepare updates directory {}: {err}",
            dir.display()
        )
    })?;

    let seq = DOWNLOAD_SEQ.fetch_add(1, Ordering::Relaxed);
    let archive_path = dir.join(format!("download-{}-{seq}.tar.gz", std::process::id()));

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive_path)
        .map_err(|err| format!("Could not create download file: {err}"))?;

    let result = (|| -> Result<(), String> {
        let response = http_get_response(&update.url)?;
        let header_len = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let total = update.size.or(header_len);

        let mut reader = response.into_body().into_reader();
        let mut verifier = Download::new(sha256, update.size);
        let mut buf = vec![0u8; DOWNLOAD_CHUNK_BYTES];
        let mut last_emit = Instant::now() - PROGRESS_THROTTLE;
        let mut received = 0u64;

        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|err| format!("Download interrupted: {err}"))?;
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            received = verifier
                .update(chunk)
                .map_err(|err| format!("Update verification failed: {err}"))?;
            file.write_all(chunk)
                .map_err(|err| format!("Could not write downloaded update: {err}"))?;
            if last_emit.elapsed() >= PROGRESS_THROTTLE {
                on_progress(received, total);
                last_emit = Instant::now();
            }
        }

        file.flush()
            .map_err(|err| format!("Could not flush downloaded update: {err}"))?;
        verifier
            .finish()
            .map_err(|err| format!("Update checksum verification failed: {err}"))?;
        on_progress(received, total);
        Ok(())
    })();

    if let Err(err) = result {
        let _ = fs::remove_file(&archive_path);
        return Err(err);
    }

    Ok(archive_path)
}

/// Replaces the installed `Bruk.app` with the downloaded archive, remembering the bundle path
/// so the caller can immediately invoke `nsh_core::element::desktop::restart()`.
/// Blocking; call from a background thread.
pub fn install_downloaded_update(archive_path: &Path) -> Result<(), String> {
    let shipped = load_shipped_config()?.ok_or_else(|| {
        "Cannot install update: Bruk is not running from a packaged .app bundle.".to_owned()
    })?;
    let installation = Installation::of(&shipped.exe, &shipped.config.id, &shipped.raw)
        .ok_or_else(|| "Cannot locate the installed Bruk.app bundle.".to_owned())?;

    // Drop any previously staged on-quit update before installing immediately.
    clear_staged_update();

    // Capture the original bundle path BEFORE swapping directories, since renaming the running
    // `.app` causes `current_exe()` to point at the moved/deleted bundle.
    nsh_core::restart::remember_program();

    installation
        .install(archive_path)
        .map_err(|err| format!("Could not install update: {err}"))?;

    let _ = fs::remove_file(archive_path);
    Ok(())
}

/// Unpacks and verifies the downloaded archive next to `Bruk.app` so it can be swapped in
/// cleanly when the user quits (`finish_staged_update_on_quit`).
/// Blocking; call from a background thread.
pub fn stage_downloaded_update_for_quit(
    version: String,
    archive_path: PathBuf,
) -> Result<(), String> {
    let shipped = load_shipped_config()?.ok_or_else(|| {
        "Cannot stage update: Bruk is not running from a packaged .app bundle.".to_owned()
    })?;
    let installation = Installation::of(&shipped.exe, &shipped.config.id, &shipped.raw)
        .ok_or_else(|| "Cannot locate the installed Bruk.app bundle.".to_owned())?;

    clear_staged_update();

    let staged = installation
        .stage(&archive_path)
        .map_err(|err| format!("Could not stage update for quit: {err}"))?;

    let mut guard = STAGED_ON_QUIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(PendingOnQuit {
        version,
        staged,
        archive_path,
    });
    Ok(())
}

/// Drops any pending staged-on-quit update (which removes its staging directory on drop).
pub fn clear_staged_update() {
    let previous = STAGED_ON_QUIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    drop(previous);
}

/// Completes a staged-on-quit update when the app exits. Safe to call unconditionally from
/// `nsh_core::element::desktop::on_app_quit`.
pub fn finish_staged_update_on_quit() {
    let pending = STAGED_ON_QUIT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    let Some(PendingOnQuit {
        version,
        staged,
        archive_path,
    }) = pending
    else {
        return;
    };

    nsh_core::restart::remember_program();
    match staged.finish() {
        Ok(()) => log::info!("updater: installed Bruk {version} on quit"),
        Err(err) => log::error!("updater: failed to install Bruk {version} on quit: {err}"),
    }
    let _ = fs::remove_file(archive_path);
}

fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .build()
            .into()
    })
}

fn check_allowed_url(url: &str) -> Result<(), String> {
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| {
            url.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([':', '/']))
        });
    if url.starts_with("https://") || loopback {
        Ok(())
    } else {
        Err(format!("Refusing insecure update URL: {url}"))
    }
}

fn http_get_response(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    let agent = http_agent();
    let user_agent = format!("Bruk/{}", env!("CARGO_PKG_VERSION"));
    let mut current = url.to_owned();

    for _ in 0..=MAX_REDIRECTS {
        check_allowed_url(&current)?;
        let response = agent
            .get(&current)
            .header("User-Agent", &user_agent)
            .call()
            .map_err(|err| format!("Network request failed: {err}"))?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| format!("Redirect ({status}) without Location header"))?;
            current = resolve_redirect(&current, location);
            continue;
        }
        if !status.is_success() {
            return Err(format!("Server returned HTTP {}", status.as_u16()));
        }
        return Ok(response);
    }

    Err("Too many HTTP redirects while fetching update.".to_owned())
}

fn resolve_redirect(base: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_owned();
    }
    if location.starts_with('/')
        && let Some(scheme_end) = base.find("://")
    {
        let authority_start = scheme_end + 3;
        let authority_end = base[authority_start..]
            .find('/')
            .map(|offset| authority_start + offset)
            .unwrap_or(base.len());
        return format!("{}{location}", &base[..authority_end]);
    }
    let path_end = base.find(['?', '#']).unwrap_or(base.len());
    match base[..path_end].rfind('/') {
        Some(slash) => format!("{}{location}", &base[..=slash]),
        None => location.to_owned(),
    }
}

fn http_get_bytes(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let response = http_get_response(url)?;
    let mut reader = response.into_body().into_reader();
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|err| format!("Failed reading response body: {err}"))?;
        if n == 0 {
            break;
        }
        if out.len() + n > max_bytes {
            return Err(format!(
                "Response exceeded maximum size ({max_bytes} bytes)"
            ));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader},
        net::TcpListener,
        sync::Arc,
        thread,
    };

    use tempfile::TempDir;

    use super::*;

    const TEST_SECRET_KEY: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
    const TEST_PUBLIC_KEY: &str = "iojj3XQJ8ZX9UtstPLpdcspnCb8dlBIb83SIAbQPb1w=";

    #[derive(Clone)]
    struct TestResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    fn start_test_server(
        routes: HashMap<String, TestResponse>,
        expected_requests: usize,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let routes = Arc::new(routes);
        let handle = thread::spawn(move || {
            for _ in 0..expected_requests {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_owned();
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                }
                let TestResponse {
                    status,
                    headers,
                    body,
                } = routes.get(&path).cloned().unwrap_or(TestResponse {
                    status: 404,
                    headers: vec![],
                    body: b"not found".to_vec(),
                });
                let reason = match status {
                    200 => "OK",
                    302 => "Found",
                    _ => "Not Found",
                };
                let mut head = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
                stream.flush().unwrap();
            }
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn resolves_shipped_config_path_inside_macos_app_bundle() {
        let exe = Path::new("/Applications/Bruk.app/Contents/MacOS/Bruk");
        let path = shipped_config_path_for_exe(exe).unwrap();
        assert_eq!(
            path,
            Path::new("/Applications/Bruk.app/Contents/Resources/nsh/assets/nsh-updater.json")
        );

        let dev_exe = Path::new("/Users/dev/project/target/debug/factory-app");
        assert!(shipped_config_path_for_exe(dev_exe).is_none());
    }

    #[test]
    fn check_and_download_verify_signature_redirect_and_sha256() {
        let payload = b"fake-tar-gz-archive-payload-for-bruk-0.2.0".to_vec();
        let sha = nsh_updater::sha256(&payload);

        let manifest = serde_json::to_vec(&serde_json::json!({
            "id": "dev.frithuz.bruk",
            "version": "0.2.0",
            "notes": "Auto-updater support",
            "platforms": {
                "macos-aarch64": {
                    "url": "Bruk-0.2.0-macos-aarch64.tar.gz",
                    "sha256": sha,
                    "size": payload.len() as u64,
                }
            }
        }))
        .unwrap();
        let signature = nsh_updater::sign(TEST_SECRET_KEY, &manifest)
            .unwrap()
            .into_bytes();

        let mut routes = HashMap::new();
        routes.insert(
            "/releases/latest/download/latest.json".to_owned(),
            TestResponse {
                status: 302,
                headers: vec![(
                    "Location".to_owned(),
                    "/releases/download/v0.2.0/latest.json".to_owned(),
                )],
                body: vec![],
            },
        );
        routes.insert(
            "/releases/download/v0.2.0/latest.json".to_owned(),
            TestResponse {
                status: 200,
                headers: vec![],
                body: manifest,
            },
        );
        routes.insert(
            "/releases/latest/download/latest.json.sig".to_owned(),
            TestResponse {
                status: 200,
                headers: vec![],
                body: signature,
            },
        );
        routes.insert(
            "/releases/latest/download/Bruk-0.2.0-macos-aarch64.tar.gz".to_owned(),
            TestResponse {
                status: 200,
                headers: vec![],
                body: payload.clone(),
            },
        );

        let (base, server) = start_test_server(routes, 4);

        let config_json = serde_json::to_vec(&serde_json::json!({
            "id": "dev.frithuz.bruk",
            "version": "0.1.0",
            "url": format!("{base}/releases/latest/download/latest.json"),
            "publicKey": TEST_PUBLIC_KEY,
        }))
        .unwrap();
        let config = Config::parse(&config_json).unwrap();

        let outcome = check_with_config(
            &config,
            "macos-aarch64",
            None,
            Path::new("/Applications/Bruk.app/Contents/MacOS/Bruk"),
        )
        .unwrap();

        let CheckOutcome::Available(update) = outcome else {
            panic!("expected Available, got {outcome:?}");
        };
        assert_eq!(update.version, "0.2.0");
        assert_eq!(update.notes.as_deref(), Some("Auto-updater support"));
        assert_eq!(
            update.url,
            format!("{base}/releases/latest/download/Bruk-0.2.0-macos-aarch64.tar.gz")
        );

        let tmp = TempDir::new().unwrap();
        let mut last_progress = (0u64, None);
        let archive = download_update_into(&update, tmp.path(), |received, total| {
            last_progress = (received, total);
        })
        .unwrap();

        assert_eq!(fs::read(&archive).unwrap(), payload);
        assert_eq!(
            last_progress,
            (payload.len() as u64, Some(payload.len() as u64))
        );
        server.join().unwrap();
    }

    #[test]
    fn download_rejects_corrupted_sha256_and_cleans_up_file() {
        let mut routes = HashMap::new();
        routes.insert(
            "/bad.tar.gz".to_owned(),
            TestResponse {
                status: 200,
                headers: vec![],
                body: b"tampered bytes".to_vec(),
            },
        );
        let (base, server) = start_test_server(routes, 1);

        let update = AvailableUpdate {
            version: "0.2.0".to_owned(),
            notes: None,
            url: format!("{base}/bad.tar.gz"),
            sha256: Some("0".repeat(64)),
            size: Some(14),
            can_install: true,
            install_blocked_reason: None,
        };

        let tmp = TempDir::new().unwrap();
        let err = download_update_into(&update, tmp.path(), |_, _| {}).unwrap_err();
        assert!(err.contains("checksum"), "unexpected error: {err}");
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
        server.join().unwrap();
    }

    #[test]
    fn translocated_app_reports_move_to_applications_reason() {
        let mut available = Available::default();
        available.version = "0.2.0".to_owned();
        available.url = "https://github.com/frithuz/Bruk/releases/latest/download/Bruk-0.2.0-macos-aarch64.tar.gz".to_owned();
        available.sha256 = Some("a".repeat(64));

        let mapped = to_available_update(
            available,
            None,
            Path::new(
                "/private/var/folders/xx/AppTranslocation/1234/d/Bruk.app/Contents/MacOS/Bruk",
            ),
        );
        assert!(!mapped.can_install);
        assert!(
            mapped
                .install_blocked_reason
                .as_deref()
                .is_some_and(|msg| msg.contains("/Applications"))
        );
    }

    #[test]
    fn format_bytes_renders_readable_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(4_096), "4 KB");
        assert_eq!(format_bytes(12 * 1024 * 1024), "12.0 MB");
    }
}
