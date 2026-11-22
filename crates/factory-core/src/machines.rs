//! Machines fleet domain model, schema compatibility, catch-up summaries, and active sessions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current Bruk desktop application version displayed in the Machines header.
pub const BRUK_APP_VERSION: &str = "0.9.0";

/// Current Bruk data schema version compared against each remote core on connection.
pub const BRUK_DATA_SCHEMA: u32 = 7;

/// Operating system family badge rendered on each machine list card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachinePlatform {
    Mac,
    Linux,
    Windows,
}

impl MachinePlatform {
    pub fn short_badge(self) -> &'static str {
        match self {
            Self::Mac => "mac",
            Self::Linux => "lnx",
            Self::Windows => "win",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Mac => "macOS",
            Self::Linux => "Linux",
            Self::Windows => "Windows",
        }
    }
}

/// Live SSH and sync state for a machine running a Bruk core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineConnectionStatus {
    Connected,
    Disconnected,
    NotSyncing,
    Unreachable,
}

impl MachineConnectionStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::NotSyncing => "Not syncing",
            Self::Unreachable => "Unreachable",
        }
    }

    pub fn is_connected(self) -> bool {
        matches!(self, Self::Connected)
    }

    pub fn is_offline(self) -> bool {
        matches!(self, Self::Disconnected | Self::Unreachable)
    }
}

/// Result of comparing a machine core's data schema against Bruk's current schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaCompatibility {
    InSync,
    CoreUpdateSuggested,
    NeedsNewerBruk,
}

impl SchemaCompatibility {
    pub fn from_schemas(bruk_schema: u32, core_schema: u32) -> Self {
        if core_schema == bruk_schema {
            Self::InSync
        } else if core_schema < bruk_schema {
            Self::CoreUpdateSuggested
        } else {
            Self::NeedsNewerBruk
        }
    }

    pub fn badge_label(self) -> Option<&'static str> {
        match self {
            Self::InSync => None,
            Self::CoreUpdateSuggested => Some("Core update suggested"),
            Self::NeedsNewerBruk => Some("Needs a newer Bruk"),
        }
    }
}

/// Visual tone for a session's `Now` column status text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionNowTone {
    Attention,
    Active,
    Error,
    Muted,
}

/// Active workflow session running on a specific machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineSession {
    pub run_number: u32,
    pub project_name: String,
    pub workflow_name: String,
    pub now_label: String,
    pub now_tone: SessionNowTone,
    pub link_label: String,
}

impl MachineSession {
    pub fn run_tag(&self) -> String {
        format!("#{}", self.run_number)
    }

    pub fn project_workflow_label(&self) -> String {
        format!("{} › {}", self.project_name, self.workflow_name)
    }
}

/// Post-reconnect catch-up summary shown in the blue info banner for a machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineCatchUpSummary {
    pub headline: String,
    pub summary: String,
    pub changes: Vec<String>,
}

/// Policy governing how Bruk proposes or applies remote core updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreUpdatePolicy {
    #[default]
    SuggestAskFirst,
    AutoWhenBehind,
    ManualOnly,
}

impl CoreUpdatePolicy {
    pub const ALL: &'static [CoreUpdatePolicy] = &[
        Self::SuggestAskFirst,
        Self::AutoWhenBehind,
        Self::ManualOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::SuggestAskFirst => "Suggest when its schema is behind · ask first",
            Self::AutoWhenBehind => "Update automatically when its schema is behind",
            Self::ManualOnly => "Manual updates only",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::SuggestAskFirst => "Suggest & ask first",
            Self::AutoWhenBehind => "Auto when behind",
            Self::ManualOnly => "Manual only",
        }
    }
}

/// CLI agent installed on a machine whose isolated Bruk sign-in can be managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineAgentKind {
    ClaudeCode,
    Codex,
}

impl MachineAgentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }
}

/// Full local or remote machine record displayed in the Machines master-detail view
/// and configured in Preferences › Machines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRecord {
    pub id: u64,
    pub name: String,
    pub platform: MachinePlatform,
    pub os_short: String,
    pub prefs_os_short: String,
    pub os_detail: String,
    pub kernel: String,
    pub architecture: String,
    pub hardware_spec: String,
    pub hardware_memory: String,
    pub hostname: String,
    pub is_local: bool,
    pub status: MachineConnectionStatus,
    pub status_detail: String,
    pub address_primary: String,
    pub address_secondary: String,
    pub ipv4: String,
    pub ipv6: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub jump_host: Option<String>,
    pub host_key_type: String,
    pub host_key_fingerprint: String,
    pub host_key_full: String,
    pub host_key_trusted_on: String,
    pub ssh_target: String,
    pub ssh_key_name: String,
    pub ssh_key_type: String,
    pub ssh_key_short_fingerprint: String,
    pub ssh_key_provider: String,
    pub ssh_key_fingerprint: String,
    pub core_version: String,
    pub core_schema: u32,
    pub core_sync_note: String,
    pub core_install_path: String,
    pub core_update_policy: CoreUpdatePolicy,
    pub claude_binary_path: String,
    pub claude_signed_in: bool,
    pub codex_binary_path: String,
    pub codex_signed_in: bool,
    pub workspace_folder: String,
    pub max_sessions_at_once: u32,
    pub sessions_subtitle: String,
    pub catch_up: Option<MachineCatchUpSummary>,
    pub sessions: Vec<MachineSession>,
    pub allowed_projects: Vec<String>,
}

impl MachineRecord {
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn session_count_label(&self) -> String {
        match self.sessions.len() {
            1 => "1 session".to_owned(),
            n => format!("{n} sessions"),
        }
    }

    pub fn compatibility(&self, bruk_schema: u32) -> SchemaCompatibility {
        SchemaCompatibility::from_schemas(bruk_schema, self.core_schema)
    }

    pub fn core_summary_label(&self) -> String {
        format!("{} · schema {}", self.core_version, self.core_schema)
    }

    pub fn allowed_projects_summary(&self) -> String {
        if self.allowed_projects.is_empty() {
            "None".to_owned()
        } else {
            self.allowed_projects.join(", ")
        }
    }

    /// Formatted host key summary row shown in Preferences › Machines › Access.
    pub fn host_key_summary(&self) -> String {
        if self.is_local {
            "Local IPC · no SSH host key".to_owned()
        } else {
            format!(
                "{} {} · trusted {}",
                self.host_key_type, self.host_key_fingerprint, self.host_key_trusted_on
            )
        }
    }

    /// Formatted SSH key label shown in Preferences › Machines › SSH key.
    pub fn ssh_key_in_use_label(&self) -> String {
        if self.is_local {
            "Direct IPC · no SSH key".to_owned()
        } else {
            format!("{} · {}", self.ssh_key_name, self.ssh_key_type)
        }
    }

    /// Detailed schema comparison line shown in Preferences › Machines › Bruk core.
    pub fn data_schema_detail(&self, bruk_schema: u32) -> String {
        if self.is_local {
            return format!("{} · bundled with this app · in sync", self.core_schema);
        }
        match self.compatibility(bruk_schema) {
            SchemaCompatibility::InSync => {
                if self.status.is_offline() {
                    format!("{} · same as this app · offline", self.core_schema)
                } else {
                    format!("{} · same as this app · in sync", self.core_schema)
                }
            }
            SchemaCompatibility::CoreUpdateSuggested => format!(
                "{} · behind this app ({bruk_schema}) · update suggested",
                self.core_schema
            ),
            SchemaCompatibility::NeedsNewerBruk => format!(
                "{} · ahead of this app ({bruk_schema}) · not syncing",
                self.core_schema
            ),
        }
    }

    /// Status line for Claude Code on this machine.
    pub fn claude_status_summary(&self) -> String {
        let auth = if self.claude_signed_in {
            "signed in inside Bruk"
        } else {
            "not signed in"
        };
        format!("{} · isolated config · {auth}", self.claude_binary_path)
    }

    /// Status line for Codex on this machine.
    pub fn codex_status_summary(&self) -> String {
        let auth = if self.codex_signed_in {
            "signed in inside Bruk"
        } else {
            "not signed in"
        };
        format!("{} · isolated config · {auth}", self.codex_binary_path)
    }
}

/// Input payload when registering a new remote SSH machine from the `+ Add Machine...` drawer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMachineDraft {
    pub name: String,
    pub platform: MachinePlatform,
    pub os_label: String,
    pub arch: String,
    pub address: String,
    pub ssh_target: String,
    pub allowed_projects: Vec<String>,
}

/// Domain errors returned when mutating the machines fleet.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MachinesError {
    #[error("machine name cannot be empty")]
    EmptyName,
    #[error("SSH target cannot be empty")]
    EmptySshTarget,
    #[error("SSH user cannot be empty")]
    EmptySshUser,
    #[error("SSH key name cannot be empty")]
    EmptySshKey,
    #[error("sessions at once must be a number between 1 and 32")]
    InvalidSessionLimit,
    #[error("machine '{0}' already exists")]
    DuplicateName(String),
    #[error("machine #{0} was not found")]
    NotFound(u64),
    #[error("the local machine cannot be disconnected or removed")]
    LocalMachineProtected,
}

/// Fleet of local and remote machines managed by Bruk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachinesFleet {
    pub bruk_version: String,
    pub bruk_schema: u32,
    pub machines: Vec<MachineRecord>,
}

impl Default for MachinesFleet {
    fn default() -> Self {
        Self::sample()
    }
}

impl MachinesFleet {
    /// Seeds the 5-machine fleet matching `21 Machines.png`.
    pub fn sample() -> Self {
        Self {
            bruk_version: BRUK_APP_VERSION.to_owned(),
            bruk_schema: BRUK_DATA_SCHEMA,
            machines: vec![
                MachineRecord {
                    id: 1,
                    name: "This Mac".to_owned(),
                    platform: MachinePlatform::Mac,
                    os_short: "macOS 26.1 · arm64".to_owned(),
                    prefs_os_short: "macOS 26.1 · arm64".to_owned(),
                    os_detail: "macOS 26.1".to_owned(),
                    kernel: "Darwin 25.1.0".to_owned(),
                    architecture: "arm64".to_owned(),
                    hardware_spec: "arm64 · 16 cores · 64 GB".to_owned(),
                    hardware_memory: "16 cores · 64 GB memory".to_owned(),
                    hostname: "jonathans-mac.local".to_owned(),
                    is_local: true,
                    status: MachineConnectionStatus::Connected,
                    status_detail: "Local".to_owned(),
                    address_primary: "localhost".to_owned(),
                    address_secondary: "Unix domain socket".to_owned(),
                    ipv4: "127.0.0.1".to_owned(),
                    ipv6: "::1".to_owned(),
                    ssh_port: 22,
                    ssh_user: "jonathan".to_owned(),
                    jump_host: None,
                    host_key_type: "LOCAL".to_owned(),
                    host_key_fingerprint: "Direct IPC".to_owned(),
                    host_key_full: "Local Unix domain socket · no SSH host key required".to_owned(),
                    host_key_trusted_on: "Local".to_owned(),
                    ssh_target: "Local transport".to_owned(),
                    ssh_key_name: "Local IPC".to_owned(),
                    ssh_key_type: "Direct".to_owned(),
                    ssh_key_short_fingerprint: "—".to_owned(),
                    ssh_key_provider: "Local Unix domain socket".to_owned(),
                    ssh_key_fingerprint: "Direct IPC · no SSH key".to_owned(),
                    core_version: "0.9.0".to_owned(),
                    core_schema: 7,
                    core_sync_note: "Bundled · in sync".to_owned(),
                    core_install_path: "/Applications/Bruk.app/Contents/MacOS/bruk-core".to_owned(),
                    core_update_policy: CoreUpdatePolicy::SuggestAskFirst,
                    claude_binary_path: "/opt/homebrew/bin/claude".to_owned(),
                    claude_signed_in: true,
                    codex_binary_path: "/opt/homebrew/bin/codex".to_owned(),
                    codex_signed_in: true,
                    workspace_folder: "/Users/jonathan/src".to_owned(),
                    max_sessions_at_once: 4,
                    sessions_subtitle: "Running locally".to_owned(),
                    catch_up: None,
                    sessions: vec![
                        MachineSession {
                            run_number: 46,
                            project_name: "api-gateway".to_owned(),
                            workflow_name: "Release prep".to_owned(),
                            now_label: "Step 4 of 6".to_owned(),
                            now_tone: SessionNowTone::Active,
                            link_label: "Local".to_owned(),
                        },
                        MachineSession {
                            run_number: 13,
                            project_name: "marketing-site".to_owned(),
                            workflow_name: "Docs refresh".to_owned(),
                            now_label: "Step 2 of 4".to_owned(),
                            now_tone: SessionNowTone::Active,
                            link_label: "Local".to_owned(),
                        },
                        MachineSession {
                            run_number: 9,
                            project_name: "mobile-app".to_owned(),
                            workflow_name: "Fix issue".to_owned(),
                            now_label: "Needs you · review".to_owned(),
                            now_tone: SessionNowTone::Attention,
                            link_label: "Local".to_owned(),
                        },
                    ],
                    allowed_projects: vec![
                        "api-gateway".to_owned(),
                        "marketing-site".to_owned(),
                        "mobile-app".to_owned(),
                    ],
                },
                MachineRecord {
                    id: 2,
                    name: "build-box-01".to_owned(),
                    platform: MachinePlatform::Linux,
                    os_short: "Ubuntu 24.04 · x86_64".to_owned(),
                    prefs_os_short: "Ubuntu 24.04 · x86_64".to_owned(),
                    os_detail: "Ubuntu 24.04.1 LTS".to_owned(),
                    kernel: "Linux 6.8.0-48".to_owned(),
                    architecture: "x86_64".to_owned(),
                    hardware_spec: "x86_64 · 16 cores · 64 GB".to_owned(),
                    hardware_memory: "16 cores · 64 GB memory".to_owned(),
                    hostname: "build-box-01".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::Connected,
                    status_detail: "Caught up 2 min ago".to_owned(),
                    address_primary: "10.0.4.21:22".to_owned(),
                    address_secondary: "fd00::4:21".to_owned(),
                    ipv4: "10.0.4.21".to_owned(),
                    ipv6: "fd00::4:21".to_owned(),
                    ssh_port: 22,
                    ssh_user: "bruk".to_owned(),
                    jump_host: None,
                    host_key_type: "ED25519".to_owned(),
                    host_key_fingerprint: "SHA256:Y7nR…2fKa".to_owned(),
                    host_key_full: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKm8v4N9zQ2pL7wX3rY1tB6cF5hJ8kM0nP4qR7sU2fKa root@build-box-01".to_owned(),
                    host_key_trusted_on: "Nov 3".to_owned(),
                    ssh_target: "bruk@build-box-01".to_owned(),
                    ssh_key_name: "bruk-build".to_owned(),
                    ssh_key_type: "ED25519".to_owned(),
                    ssh_key_short_fingerprint: "SHA256:q3Ff…8kPw".to_owned(),
                    ssh_key_provider: "SSH agent (macOS Keychain)".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:q3Ff…8k…".to_owned(),
                    core_version: "0.8.5".to_owned(),
                    core_schema: 7,
                    core_sync_note: "Same schema · in sync".to_owned(),
                    core_install_path: "/home/bruk/.bruk/core".to_owned(),
                    core_update_policy: CoreUpdatePolicy::SuggestAskFirst,
                    claude_binary_path: "/usr/local/bin/claude".to_owned(),
                    claude_signed_in: true,
                    codex_binary_path: "/home/bruk/.local/bin/codex".to_owned(),
                    codex_signed_in: true,
                    workspace_folder: "/home/bruk/work".to_owned(),
                    max_sessions_at_once: 4,
                    sessions_subtitle: "Live again after catching up".to_owned(),
                    catch_up: Some(MachineCatchUpSummary {
                        headline: "Reconnected 2 minutes ago and caught up".to_owned(),
                        summary: "While Bruk was away, the 4 sessions here produced 318 events: 3 steps finished, one loop ran out of attempts, and 2 new requests are in the Inbox.".to_owned(),
                        changes: vec![
                            "#44 api-gateway › Dependency upgrade — paused for permission in Inbox (214 events)".to_owned(),
                            "#12 marketing-site › Docs refresh — asked a clarifying question in Inbox (46 events)".to_owned(),
                            "#7 mobile-app › Dependency upgrade — retry loop reached max attempts (39 events)".to_owned(),
                            "#45 api-gateway › Fix issue — completed step setup, now on Step 1 of 6 (19 events)".to_owned(),
                        ],
                    }),
                    sessions: vec![
                        MachineSession {
                            run_number: 44,
                            project_name: "api-gateway".to_owned(),
                            workflow_name: "Dependency upgrade".to_owned(),
                            now_label: "Needs you · permission".to_owned(),
                            now_tone: SessionNowTone::Attention,
                            link_label: "Streaming".to_owned(),
                        },
                        MachineSession {
                            run_number: 12,
                            project_name: "marketing-site".to_owned(),
                            workflow_name: "Docs refresh".to_owned(),
                            now_label: "Needs you · question".to_owned(),
                            now_tone: SessionNowTone::Attention,
                            link_label: "Streaming".to_owned(),
                        },
                        MachineSession {
                            run_number: 7,
                            project_name: "mobile-app".to_owned(),
                            workflow_name: "Dependency upgrade".to_owned(),
                            now_label: "Out of attempts".to_owned(),
                            now_tone: SessionNowTone::Attention,
                            link_label: "Streaming".to_owned(),
                        },
                        MachineSession {
                            run_number: 45,
                            project_name: "api-gateway".to_owned(),
                            workflow_name: "Fix issue".to_owned(),
                            now_label: "Step 1 of 6".to_owned(),
                            now_tone: SessionNowTone::Active,
                            link_label: "Streaming".to_owned(),
                        },
                    ],
                    allowed_projects: vec![
                        "api-gateway".to_owned(),
                        "marketing-site".to_owned(),
                        "mobile-app".to_owned(),
                    ],
                },
                MachineRecord {
                    id: 3,
                    name: "win-ci-02".to_owned(),
                    platform: MachinePlatform::Windows,
                    os_short: "Windows 11 24H2 · x86_64".to_owned(),
                    prefs_os_short: "Windows 11 · x86_64".to_owned(),
                    os_detail: "Windows 11 24H2".to_owned(),
                    kernel: "Windows NT 10.0.26100".to_owned(),
                    architecture: "x86_64".to_owned(),
                    hardware_spec: "x86_64 · 12 cores · 32 GB".to_owned(),
                    hardware_memory: "12 cores · 32 GB memory".to_owned(),
                    hostname: "win-ci-02".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::Disconnected,
                    status_detail: "Last seen 3 h ago".to_owned(),
                    address_primary: "10.0.4.29:22".to_owned(),
                    address_secondary: "fd00::4:29".to_owned(),
                    ipv4: "10.0.4.29".to_owned(),
                    ipv6: "fd00::4:29".to_owned(),
                    ssh_port: 22,
                    ssh_user: "bruk".to_owned(),
                    jump_host: None,
                    host_key_type: "ED25519".to_owned(),
                    host_key_fingerprint: "SHA256:K4mP…9dLx".to_owned(),
                    host_key_full: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPw7nR2xK4mP9dLxV8cQ1bF3hT6yJ0uE5aS8zN4m9dLx bruk@win-ci-02".to_owned(),
                    host_key_trusted_on: "Oct 19".to_owned(),
                    ssh_target: "bruk@win-ci-02".to_owned(),
                    ssh_key_name: "bruk-ci".to_owned(),
                    ssh_key_type: "ED25519".to_owned(),
                    ssh_key_short_fingerprint: "SHA256:k8Lm…2pRt".to_owned(),
                    ssh_key_provider: "SSH agent (macOS Keychain)".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:k8Lm…2p…".to_owned(),
                    core_version: "0.8.3".to_owned(),
                    core_schema: 6,
                    core_sync_note: "Older schema · update suggested".to_owned(),
                    core_install_path: "C:\\Users\\bruk\\.bruk\\core".to_owned(),
                    core_update_policy: CoreUpdatePolicy::SuggestAskFirst,
                    claude_binary_path: "C:\\Program Files\\Claude\\claude.exe".to_owned(),
                    claude_signed_in: true,
                    codex_binary_path: "C:\\Users\\bruk\\.local\\bin\\codex.exe".to_owned(),
                    codex_signed_in: true,
                    workspace_folder: "C:\\bruk\\work".to_owned(),
                    max_sessions_at_once: 2,
                    sessions_subtitle: "Continuing on host while disconnected".to_owned(),
                    catch_up: None,
                    sessions: vec![
                        MachineSession {
                            run_number: 31,
                            project_name: "api-gateway".to_owned(),
                            workflow_name: "Fix issue".to_owned(),
                            now_label: "Running remotely".to_owned(),
                            now_tone: SessionNowTone::Muted,
                            link_label: "Disconnected".to_owned(),
                        },
                        MachineSession {
                            run_number: 18,
                            project_name: "mobile-app".to_owned(),
                            workflow_name: "Release prep".to_owned(),
                            now_label: "Step 3 of 5".to_owned(),
                            now_tone: SessionNowTone::Active,
                            link_label: "Disconnected".to_owned(),
                        },
                    ],
                    allowed_projects: vec!["api-gateway".to_owned(), "mobile-app".to_owned()],
                },
                MachineRecord {
                    id: 4,
                    name: "gpu-01".to_owned(),
                    platform: MachinePlatform::Linux,
                    os_short: "Fedora 41 · x86_64".to_owned(),
                    prefs_os_short: "Fedora 41 · x86_64".to_owned(),
                    os_detail: "Fedora 41 Workstation".to_owned(),
                    kernel: "Linux 6.12.7-200.fc41".to_owned(),
                    architecture: "x86_64".to_owned(),
                    hardware_spec: "x86_64 · 32 cores · 128 GB".to_owned(),
                    hardware_memory: "32 cores · 128 GB memory".to_owned(),
                    hostname: "gpu-01".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::NotSyncing,
                    status_detail: "Connected 5 min ago".to_owned(),
                    address_primary: "10.0.4.44:22".to_owned(),
                    address_secondary: "fd00::4:44".to_owned(),
                    ipv4: "10.0.4.44".to_owned(),
                    ipv6: "fd00::4:44".to_owned(),
                    ssh_port: 22,
                    ssh_user: "bruk".to_owned(),
                    jump_host: Some("bastion.lab.internal".to_owned()),
                    host_key_type: "ED25519".to_owned(),
                    host_key_fingerprint: "SHA256:R9tV…3nQz".to_owned(),
                    host_key_full: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIR9tV3nQzB2mK8wL5pX1cF7jN0dH4yE6qA9sU3nQz root@gpu-01".to_owned(),
                    host_key_trusted_on: "Nov 11".to_owned(),
                    ssh_target: "bruk@gpu-01".to_owned(),
                    ssh_key_name: "bruk-gpu".to_owned(),
                    ssh_key_type: "ED25519".to_owned(),
                    ssh_key_short_fingerprint: "SHA256:x9Rt…4nBv".to_owned(),
                    ssh_key_provider: "SSH agent (macOS Keychain)".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:x9Rt…4n…".to_owned(),
                    core_version: "1.0.0".to_owned(),
                    core_schema: 8,
                    core_sync_note: "Newer schema · not syncing".to_owned(),
                    core_install_path: "/home/bruk/.bruk/core".to_owned(),
                    core_update_policy: CoreUpdatePolicy::SuggestAskFirst,
                    claude_binary_path: "/usr/local/bin/claude".to_owned(),
                    claude_signed_in: true,
                    codex_binary_path: "/home/bruk/.local/bin/codex".to_owned(),
                    codex_signed_in: false,
                    workspace_folder: "/home/bruk/work".to_owned(),
                    max_sessions_at_once: 6,
                    sessions_subtitle: "Paused sync until Bruk is updated".to_owned(),
                    catch_up: None,
                    sessions: vec![MachineSession {
                        run_number: 5,
                        project_name: "api-gateway".to_owned(),
                        workflow_name: "Release prep".to_owned(),
                        now_label: "Sync paused".to_owned(),
                        now_tone: SessionNowTone::Error,
                        link_label: "Not syncing".to_owned(),
                    }],
                    allowed_projects: vec!["api-gateway".to_owned()],
                },
                MachineRecord {
                    id: 5,
                    name: "mini-lab".to_owned(),
                    platform: MachinePlatform::Mac,
                    os_short: "macOS 15.6 · arm64".to_owned(),
                    prefs_os_short: "macOS 15.6 · arm64".to_owned(),
                    os_detail: "macOS 15.6".to_owned(),
                    kernel: "Darwin 24.6.0".to_owned(),
                    architecture: "arm64".to_owned(),
                    hardware_spec: "arm64 · 10 cores · 24 GB".to_owned(),
                    hardware_memory: "10 cores · 24 GB memory".to_owned(),
                    hostname: "mini-lab.local".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::Unreachable,
                    status_detail: "Since Sunday".to_owned(),
                    address_primary: "10.0.4.38:22".to_owned(),
                    address_secondary: "fd00::4:38".to_owned(),
                    ipv4: "10.0.4.38".to_owned(),
                    ipv6: "fd00::4:38".to_owned(),
                    ssh_port: 22,
                    ssh_user: "bruk".to_owned(),
                    jump_host: None,
                    host_key_type: "ED25519".to_owned(),
                    host_key_fingerprint: "SHA256:M2vX…7qHs".to_owned(),
                    host_key_full: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM2vX7qHsT4kP9wN1bC6fR8jL3dY5zE0uA2mQ7qHs bruk@mini-lab".to_owned(),
                    host_key_trusted_on: "Oct 28".to_owned(),
                    ssh_target: "bruk@mini-lab".to_owned(),
                    ssh_key_name: "bruk-lab".to_owned(),
                    ssh_key_type: "ED25519".to_owned(),
                    ssh_key_short_fingerprint: "SHA256:m2Vx…9qKd".to_owned(),
                    ssh_key_provider: "SSH agent (macOS Keychain)".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:m2Vx…9q…".to_owned(),
                    core_version: "0.9.0".to_owned(),
                    core_schema: 7,
                    core_sync_note: "Same schema · offline".to_owned(),
                    core_install_path: "/Users/bruk/.bruk/core".to_owned(),
                    core_update_policy: CoreUpdatePolicy::SuggestAskFirst,
                    claude_binary_path: "/opt/homebrew/bin/claude".to_owned(),
                    claude_signed_in: true,
                    codex_binary_path: "/Users/bruk/.local/bin/codex".to_owned(),
                    codex_signed_in: true,
                    workspace_folder: "/Users/bruk/work".to_owned(),
                    max_sessions_at_once: 2,
                    sessions_subtitle: "No active sessions".to_owned(),
                    catch_up: None,
                    sessions: Vec::new(),
                    allowed_projects: vec![
                        "api-gateway".to_owned(),
                        "marketing-site".to_owned(),
                    ],
                },
            ],
        }
    }

    /// Selects `build-box-01` by default, matching `21 Machines.png`.
    pub fn default_selected_id(&self) -> Option<u64> {
        self.machines
            .iter()
            .find(|m| m.name == "build-box-01")
            .or_else(|| self.machines.first())
            .map(|m| m.id)
    }

    pub fn machine(&self, id: u64) -> Option<&MachineRecord> {
        self.machines.iter().find(|m| m.id == id)
    }

    pub fn total_sessions(&self) -> usize {
        self.machines.iter().map(MachineRecord::session_count).sum()
    }

    pub fn connected_count(&self) -> usize {
        self.machines
            .iter()
            .filter(|m| m.status.is_connected())
            .count()
    }

    pub fn offline_count(&self) -> usize {
        self.machines
            .iter()
            .filter(|m| m.status.is_offline())
            .count()
    }

    pub fn core_update_suggested_count(&self) -> usize {
        self.machines
            .iter()
            .filter(|m| {
                m.compatibility(self.bruk_schema) == SchemaCompatibility::CoreUpdateSuggested
            })
            .count()
    }

    pub fn needs_newer_bruk_count(&self) -> usize {
        self.machines
            .iter()
            .filter(|m| m.compatibility(self.bruk_schema) == SchemaCompatibility::NeedsNewerBruk)
            .count()
    }

    /// Formats the unified top-bar subtitle:
    /// `5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Bruk`
    pub fn header_subtitle(&self) -> String {
        let machine_count = self.machines.len();
        let machine_word = if machine_count == 1 {
            "machine"
        } else {
            "machines"
        };
        let session_count = self.total_sessions();
        let session_word = if session_count == 1 {
            "session"
        } else {
            "sessions"
        };

        let mut parts = vec![
            format!("{machine_count} {machine_word}"),
            format!("{session_count} {session_word}"),
            format!(
                "Bruk {} (data schema {})",
                self.bruk_version, self.bruk_schema
            ),
        ];

        let updates = self.core_update_suggested_count();
        if updates == 1 {
            parts.push("1 core update suggested".to_owned());
        } else if updates > 1 {
            parts.push(format!("{updates} core updates suggested"));
        }

        let newer = self.needs_newer_bruk_count();
        if newer == 1 {
            parts.push("1 machine needs a newer Bruk".to_owned());
        } else if newer > 1 {
            parts.push(format!("{newer} machines need a newer Bruk"));
        }

        parts.join(" · ")
    }

    pub fn disconnect_machine(&mut self, id: u64) -> Result<&MachineRecord, MachinesError> {
        let idx = self
            .machines
            .iter()
            .position(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        if self.machines[idx].is_local {
            return Err(MachinesError::LocalMachineProtected);
        }
        let bruk_schema = self.bruk_schema;
        let machine = &mut self.machines[idx];
        machine.status = MachineConnectionStatus::Disconnected;
        machine.status_detail = "Disconnected just now".to_owned();
        machine.catch_up = None;
        machine.sessions_subtitle = if machine.sessions.is_empty() {
            "No active sessions".to_owned()
        } else {
            "Continuing on host while disconnected".to_owned()
        };
        for session in &mut machine.sessions {
            session.link_label = "Disconnected".to_owned();
        }
        machine.core_sync_note = match machine.compatibility(bruk_schema) {
            SchemaCompatibility::InSync => "Same schema · offline".to_owned(),
            SchemaCompatibility::CoreUpdateSuggested => {
                "Older schema · update suggested".to_owned()
            }
            SchemaCompatibility::NeedsNewerBruk => "Newer schema · not syncing".to_owned(),
        };
        Ok(&self.machines[idx])
    }

    pub fn reconnect_machine(&mut self, id: u64) -> Result<&MachineRecord, MachinesError> {
        let idx = self
            .machines
            .iter()
            .position(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        let bruk_schema = self.bruk_schema;
        let machine = &mut self.machines[idx];
        let compat = machine.compatibility(bruk_schema);
        match compat {
            SchemaCompatibility::NeedsNewerBruk => {
                machine.status = MachineConnectionStatus::NotSyncing;
                machine.status_detail = "Connected just now".to_owned();
                machine.core_sync_note = "Newer schema · not syncing".to_owned();
                machine.sessions_subtitle = "Paused sync until Bruk is updated".to_owned();
                for session in &mut machine.sessions {
                    session.link_label = "Not syncing".to_owned();
                }
            }
            SchemaCompatibility::InSync | SchemaCompatibility::CoreUpdateSuggested => {
                machine.status = MachineConnectionStatus::Connected;
                machine.status_detail = "Caught up just now".to_owned();
                machine.core_sync_note = if compat == SchemaCompatibility::InSync {
                    "Same schema · in sync".to_owned()
                } else {
                    "Older schema · update suggested".to_owned()
                };
                machine.sessions_subtitle = if machine.sessions.is_empty() {
                    "Ready for new sessions".to_owned()
                } else {
                    "Live again after catching up".to_owned()
                };
                for session in &mut machine.sessions {
                    session.link_label = "Streaming".to_owned();
                }
            }
        }
        Ok(&self.machines[idx])
    }

    pub fn update_core(&mut self, id: u64) -> Result<&MachineRecord, MachinesError> {
        let idx = self
            .machines
            .iter()
            .position(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        let target_version = self.bruk_version.clone();
        let target_schema = self.bruk_schema;
        let machine = &mut self.machines[idx];
        machine.core_version = target_version;
        machine.core_schema = target_schema;
        machine.core_sync_note = if machine.status.is_connected() {
            "Same schema · in sync".to_owned()
        } else {
            "Same schema · offline".to_owned()
        };
        Ok(&self.machines[idx])
    }

    /// Updates every machine whose core schema is older than Bruk's current schema.
    pub fn update_all_suggested_cores(&mut self) -> usize {
        let bruk_version = self.bruk_version.clone();
        let bruk_schema = self.bruk_schema;
        let mut updated = 0;
        for machine in &mut self.machines {
            if machine.compatibility(bruk_schema) == SchemaCompatibility::CoreUpdateSuggested {
                machine.core_version = bruk_version.clone();
                machine.core_schema = bruk_schema;
                machine.core_sync_note = if machine.status.is_connected() {
                    "Same schema · in sync".to_owned()
                } else {
                    "Same schema · offline".to_owned()
                };
                updated += 1;
            }
        }
        updated
    }

    pub fn add_machine(&mut self, draft: NewMachineDraft) -> Result<&MachineRecord, MachinesError> {
        let name = draft.name.trim().to_owned();
        if name.is_empty() {
            return Err(MachinesError::EmptyName);
        }
        if self
            .machines
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(&name))
        {
            return Err(MachinesError::DuplicateName(name));
        }

        let ssh_target = draft.ssh_target.trim().to_owned();
        if ssh_target.is_empty() {
            return Err(MachinesError::EmptySshTarget);
        }

        let next_id = self
            .machines
            .iter()
            .map(|m| m.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        let os_label = if draft.os_label.trim().is_empty() {
            match draft.platform {
                MachinePlatform::Mac => "macOS 26.1".to_owned(),
                MachinePlatform::Linux => "Ubuntu 24.04".to_owned(),
                MachinePlatform::Windows => "Windows 11 24H2".to_owned(),
            }
        } else {
            draft.os_label.trim().to_owned()
        };

        let arch = if draft.arch.trim().is_empty() {
            match draft.platform {
                MachinePlatform::Mac => "arm64".to_owned(),
                MachinePlatform::Linux | MachinePlatform::Windows => "x86_64".to_owned(),
            }
        } else {
            draft.arch.trim().to_owned()
        };

        let address_primary = if draft.address.trim().is_empty() {
            format!("10.0.4.{}:22", 50 + next_id)
        } else {
            draft.address.trim().to_owned()
        };

        let allowed_projects = if draft.allowed_projects.is_empty() {
            vec![
                "api-gateway".to_owned(),
                "marketing-site".to_owned(),
                "mobile-app".to_owned(),
            ]
        } else {
            draft.allowed_projects
        };

        let (ipv4, ssh_port) = if let Some((host, port_str)) = address_primary.rsplit_once(':')
            && let Ok(port) = port_str.parse::<u16>()
        {
            (host.to_owned(), port)
        } else {
            (address_primary.clone(), 22)
        };

        let ssh_user = ssh_target
            .split_once('@')
            .map(|(u, _)| u.trim().to_owned())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "bruk".to_owned());

        let kernel = match draft.platform {
            MachinePlatform::Mac => "Darwin 25.1.0".to_owned(),
            MachinePlatform::Linux => "Linux 6.8.0-48".to_owned(),
            MachinePlatform::Windows => "Windows NT 10.0.26100".to_owned(),
        };

        let (core_install_path, workspace_folder, claude_binary_path, codex_binary_path) =
            match draft.platform {
                MachinePlatform::Mac => (
                    format!("/Users/{ssh_user}/.bruk/core"),
                    format!("/Users/{ssh_user}/work"),
                    "/opt/homebrew/bin/claude".to_owned(),
                    format!("/Users/{ssh_user}/.local/bin/codex"),
                ),
                MachinePlatform::Linux => (
                    format!("/home/{ssh_user}/.bruk/core"),
                    format!("/home/{ssh_user}/work"),
                    "/usr/local/bin/claude".to_owned(),
                    format!("/home/{ssh_user}/.local/bin/codex"),
                ),
                MachinePlatform::Windows => (
                    format!("C:\\Users\\{ssh_user}\\.bruk\\core"),
                    "C:\\bruk\\work".to_owned(),
                    "C:\\Program Files\\Claude\\claude.exe".to_owned(),
                    format!("C:\\Users\\{ssh_user}\\.local\\bin\\codex.exe"),
                ),
            };

        let prefs_os_short = if os_label.starts_with("Windows 11") {
            format!("Windows 11 · {arch}")
        } else {
            format!("{os_label} · {arch}")
        };

        let record = MachineRecord {
            id: next_id,
            name: name.clone(),
            platform: draft.platform,
            os_short: format!("{os_label} · {arch}"),
            prefs_os_short,
            os_detail: os_label,
            kernel,
            architecture: arch.clone(),
            hardware_spec: format!("{arch} · 8 cores · 32 GB"),
            hardware_memory: "8 cores · 32 GB memory".to_owned(),
            hostname: name.clone(),
            is_local: false,
            status: MachineConnectionStatus::Connected,
            status_detail: "Connected just now".to_owned(),
            address_primary,
            address_secondary: format!("fd00::4:{}", 50 + next_id),
            ipv4,
            ipv6: format!("fd00::4:{}", 50 + next_id),
            ssh_port,
            ssh_user,
            jump_host: None,
            host_key_type: "ED25519".to_owned(),
            host_key_fingerprint: format!("SHA256:H{next_id}kP…4tWz"),
            host_key_full: format!(
                "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIH{next_id}kP4tWzQ9mL2vN8xR5cF1jB7yE3uA6sK4tWz root@{name}"
            ),
            host_key_trusted_on: "Today".to_owned(),
            ssh_target,
            ssh_key_name: "bruk-build".to_owned(),
            ssh_key_type: "ED25519".to_owned(),
            ssh_key_short_fingerprint: format!("SHA256:n{next_id}Wk…7cPm"),
            ssh_key_provider: "SSH agent (macOS Keychain)".to_owned(),
            ssh_key_fingerprint: format!("ED25519 · SHA256:n{next_id}Wk…7c…"),
            core_version: self.bruk_version.clone(),
            core_schema: self.bruk_schema,
            core_sync_note: "Same schema · in sync".to_owned(),
            core_install_path,
            core_update_policy: CoreUpdatePolicy::SuggestAskFirst,
            claude_binary_path,
            claude_signed_in: true,
            codex_binary_path,
            codex_signed_in: true,
            workspace_folder,
            max_sessions_at_once: 4,
            sessions_subtitle: "Ready for new sessions".to_owned(),
            catch_up: None,
            sessions: Vec::new(),
            allowed_projects,
        };

        self.machines.push(record);
        Ok(self.machines.last().expect("machine was just pushed"))
    }

    pub fn test_connection(&mut self, id: u64) -> Result<String, MachinesError> {
        let idx = self
            .machines
            .iter()
            .position(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        let bruk_schema = self.bruk_schema;
        let machine = &mut self.machines[idx];
        if machine.is_local {
            machine.status = MachineConnectionStatus::Connected;
            return Ok(format!(
                "Local Bruk core {} (schema {}) responded in 2 ms.",
                machine.core_version, machine.core_schema
            ));
        }

        let compat = machine.compatibility(bruk_schema);
        match compat {
            SchemaCompatibility::NeedsNewerBruk => {
                machine.status = MachineConnectionStatus::NotSyncing;
                machine.status_detail = "Checked just now".to_owned();
                Ok(format!(
                    "SSH connected to {} ({}), but core schema {} needs a newer Bruk.",
                    machine.name, machine.ipv4, machine.core_schema
                ))
            }
            SchemaCompatibility::InSync | SchemaCompatibility::CoreUpdateSuggested => {
                machine.status = MachineConnectionStatus::Connected;
                machine.status_detail = "Verified just now".to_owned();
                machine.core_sync_note = if compat == SchemaCompatibility::InSync {
                    "Same schema · in sync".to_owned()
                } else {
                    "Older schema · update suggested".to_owned()
                };
                for session in &mut machine.sessions {
                    session.link_label = "Streaming".to_owned();
                }
                Ok(format!(
                    "SSH connected to {}@{} (14 ms) · Bruk core {} (schema {}).",
                    machine.ssh_user, machine.ipv4, machine.core_version, machine.core_schema
                ))
            }
        }
    }

    pub fn set_ssh_user(&mut self, id: u64, user: &str) -> Result<(), MachinesError> {
        let trimmed = user.trim();
        if trimmed.is_empty() {
            return Err(MachinesError::EmptySshUser);
        }
        let machine = self
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        machine.ssh_user = trimmed.to_owned();
        if !machine.is_local {
            machine.ssh_target = format!("{trimmed}@{}", machine.hostname);
        }
        Ok(())
    }

    pub fn set_jump_host(&mut self, id: u64, jump_host: &str) -> Result<(), MachinesError> {
        let machine = self
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        let trimmed = jump_host.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
            machine.jump_host = None;
        } else {
            machine.jump_host = Some(trimmed.to_owned());
        }
        Ok(())
    }

    pub fn set_ssh_key(
        &mut self,
        id: u64,
        key_name: &str,
        key_type: &str,
        fingerprint: &str,
    ) -> Result<(), MachinesError> {
        let name = key_name.trim();
        if name.is_empty() {
            return Err(MachinesError::EmptySshKey);
        }
        let machine = self
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        let ktype = if key_type.trim().is_empty() {
            "ED25519"
        } else {
            key_type.trim()
        };
        let fp = if fingerprint.trim().is_empty() {
            "SHA256:k9Np…4vRm"
        } else {
            fingerprint.trim()
        };
        machine.ssh_key_name = name.to_owned();
        machine.ssh_key_type = ktype.to_owned();
        machine.ssh_key_short_fingerprint = fp.to_owned();
        machine.ssh_key_fingerprint = format!("{ktype} · {fp}");
        Ok(())
    }

    pub fn check_core_now(&mut self, id: u64) -> Result<String, MachinesError> {
        let bruk_schema = self.bruk_schema;
        let bruk_version = self.bruk_version.clone();
        let machine = self
            .machines
            .iter()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        let summary = match machine.compatibility(bruk_schema) {
            SchemaCompatibility::InSync => format!(
                "Core {} on {} matches data schema {bruk_schema} — no update needed.",
                machine.core_version, machine.name
            ),
            SchemaCompatibility::CoreUpdateSuggested => format!(
                "Core {} (schema {}) is behind Bruk {bruk_version} (schema {bruk_schema}) — update suggested.",
                machine.core_version, machine.core_schema
            ),
            SchemaCompatibility::NeedsNewerBruk => format!(
                "Core {} uses schema {}, which is ahead of Bruk {bruk_version} (schema {bruk_schema}).",
                machine.core_version, machine.core_schema
            ),
        };
        Ok(summary)
    }

    pub fn set_core_update_policy(
        &mut self,
        id: u64,
        policy: CoreUpdatePolicy,
    ) -> Result<(), MachinesError> {
        let machine = self
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        machine.core_update_policy = policy;
        Ok(())
    }

    pub fn set_agent_signed_in(
        &mut self,
        id: u64,
        agent: MachineAgentKind,
        signed_in: bool,
    ) -> Result<(), MachinesError> {
        let machine = self
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        match agent {
            MachineAgentKind::ClaudeCode => machine.claude_signed_in = signed_in,
            MachineAgentKind::Codex => machine.codex_signed_in = signed_in,
        }
        Ok(())
    }

    pub fn set_max_sessions_at_once(
        &mut self,
        id: u64,
        max_sessions: u32,
    ) -> Result<(), MachinesError> {
        if !(1..=32).contains(&max_sessions) {
            return Err(MachinesError::InvalidSessionLimit);
        }
        let machine = self
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        machine.max_sessions_at_once = max_sessions;
        Ok(())
    }

    pub fn toggle_project_access(
        &mut self,
        id: u64,
        project_name: &str,
    ) -> Result<bool, MachinesError> {
        let machine = self
            .machines
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        if let Some(pos) = machine
            .allowed_projects
            .iter()
            .position(|p| p == project_name)
        {
            machine.allowed_projects.remove(pos);
            Ok(false)
        } else {
            machine.allowed_projects.push(project_name.to_owned());
            Ok(true)
        }
    }

    pub fn remove_machine(&mut self, id: u64) -> Result<MachineRecord, MachinesError> {
        let idx = self
            .machines
            .iter()
            .position(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        if self.machines[idx].is_local {
            return Err(MachinesError::LocalMachineProtected);
        }
        Ok(self.machines.remove(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_fleet_matches_reference_mockup() {
        let fleet = MachinesFleet::sample();
        assert_eq!(fleet.machines.len(), 5);
        assert_eq!(fleet.total_sessions(), 10);
        assert_eq!(fleet.core_update_suggested_count(), 1);
        assert_eq!(fleet.needs_newer_bruk_count(), 1);
        assert_eq!(
            fleet.header_subtitle(),
            "5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Bruk"
        );
        assert_eq!(fleet.default_selected_id(), Some(2));

        let build_box = fleet.machine(2).expect("build-box-01 exists");
        assert_eq!(build_box.name, "build-box-01");
        assert_eq!(build_box.sessions.len(), 4);
        assert!(build_box.catch_up.is_some());
    }

    #[test]
    fn disconnect_and_reconnect_update_machine_and_session_links() {
        let mut fleet = MachinesFleet::sample();
        fleet.disconnect_machine(2).unwrap();
        let build_box = fleet.machine(2).unwrap();
        assert_eq!(build_box.status, MachineConnectionStatus::Disconnected);
        assert!(build_box.catch_up.is_none());
        assert!(
            build_box
                .sessions
                .iter()
                .all(|s| s.link_label == "Disconnected")
        );

        fleet.reconnect_machine(2).unwrap();
        let build_box = fleet.machine(2).unwrap();
        assert_eq!(build_box.status, MachineConnectionStatus::Connected);
        assert!(
            build_box
                .sessions
                .iter()
                .all(|s| s.link_label == "Streaming")
        );
    }

    #[test]
    fn updating_suggested_cores_clears_header_warning() {
        let mut fleet = MachinesFleet::sample();
        assert_eq!(fleet.core_update_suggested_count(), 1);

        let updated = fleet.update_all_suggested_cores();
        assert_eq!(updated, 1);
        assert_eq!(fleet.core_update_suggested_count(), 0);
        assert_eq!(
            fleet.header_subtitle(),
            "5 machines · 10 sessions · Bruk 0.9.0 (data schema 7) · 1 machine needs a newer Bruk"
        );
        assert_eq!(fleet.machine(3).unwrap().core_version, "0.9.0");
        assert_eq!(fleet.machine(3).unwrap().core_schema, 7);
    }

    #[test]
    fn adding_and_removing_machine_validates_and_protects_local_host() {
        let mut fleet = MachinesFleet::sample();
        assert_eq!(
            fleet.disconnect_machine(1),
            Err(MachinesError::LocalMachineProtected)
        );
        assert_eq!(
            fleet.remove_machine(1),
            Err(MachinesError::LocalMachineProtected)
        );

        let added = fleet
            .add_machine(NewMachineDraft {
                name: "ci-arm-03".to_owned(),
                platform: MachinePlatform::Linux,
                os_label: "Ubuntu 24.04".to_owned(),
                arch: "arm64".to_owned(),
                address: "10.0.4.60:22".to_owned(),
                ssh_target: "bruk@ci-arm-03".to_owned(),
                allowed_projects: vec!["api-gateway".to_owned()],
            })
            .unwrap();
        let added_id = added.id;
        assert_eq!(added.name, "ci-arm-03");
        assert_eq!(fleet.machines.len(), 6);

        assert!(fleet.toggle_project_access(added_id, "mobile-app").unwrap());
        assert!(
            fleet
                .machine(added_id)
                .unwrap()
                .allowed_projects
                .contains(&"mobile-app".to_owned())
        );

        let removed = fleet.remove_machine(added_id).unwrap();
        assert_eq!(removed.name, "ci-arm-03");
        assert_eq!(fleet.machines.len(), 5);
    }

    #[test]
    fn preferences_machines_metadata_and_mutations_work() {
        let mut fleet = MachinesFleet::sample();
        let build_box = fleet.machine(2).unwrap();
        assert_eq!(build_box.prefs_os_short, "Ubuntu 24.04 · x86_64");
        assert_eq!(build_box.os_detail, "Ubuntu 24.04.1 LTS");
        assert_eq!(build_box.kernel, "Linux 6.8.0-48");
        assert_eq!(build_box.architecture, "x86_64");
        assert_eq!(build_box.hardware_memory, "16 cores · 64 GB memory");
        assert_eq!(build_box.hostname, "build-box-01");
        assert_eq!(build_box.ipv4, "10.0.4.21");
        assert_eq!(build_box.ipv6, "fd00::4:21");
        assert_eq!(build_box.ssh_port, 22);
        assert_eq!(build_box.ssh_user, "bruk");
        assert_eq!(build_box.jump_host, None);
        assert_eq!(
            build_box.host_key_summary(),
            "ED25519 SHA256:Y7nR…2fKa · trusted Nov 3"
        );
        assert_eq!(build_box.ssh_key_in_use_label(), "bruk-build · ED25519");
        assert_eq!(build_box.ssh_key_short_fingerprint, "SHA256:q3Ff…8kPw");
        assert_eq!(build_box.ssh_key_provider, "SSH agent (macOS Keychain)");
        assert_eq!(build_box.core_version, "0.8.5");
        assert_eq!(
            build_box.data_schema_detail(fleet.bruk_schema),
            "7 · same as this app · in sync"
        );
        assert_eq!(build_box.core_install_path, "/home/bruk/.bruk/core");
        assert_eq!(
            build_box.core_update_policy.label(),
            "Suggest when its schema is behind · ask first"
        );
        assert_eq!(
            build_box.claude_status_summary(),
            "/usr/local/bin/claude · isolated config · signed in inside Bruk"
        );
        assert_eq!(
            build_box.codex_status_summary(),
            "/home/bruk/.local/bin/codex · isolated config · signed in inside Bruk"
        );
        assert_eq!(build_box.workspace_folder, "/home/bruk/work");
        assert_eq!(build_box.max_sessions_at_once, 4);

        fleet.set_ssh_user(2, "builder").unwrap();
        assert_eq!(fleet.machine(2).unwrap().ssh_user, "builder");
        assert_eq!(fleet.machine(2).unwrap().ssh_target, "builder@build-box-01");

        fleet.set_jump_host(2, "bastion.internal").unwrap();
        assert_eq!(
            fleet.machine(2).unwrap().jump_host.as_deref(),
            Some("bastion.internal")
        );
        fleet.set_jump_host(2, "").unwrap();
        assert_eq!(fleet.machine(2).unwrap().jump_host, None);

        fleet
            .set_ssh_key(2, "bruk-deploy", "ED25519", "SHA256:z8Kp…1mQr")
            .unwrap();
        assert_eq!(
            fleet.machine(2).unwrap().ssh_key_in_use_label(),
            "bruk-deploy · ED25519"
        );

        fleet
            .set_core_update_policy(2, CoreUpdatePolicy::AutoWhenBehind)
            .unwrap();
        assert_eq!(
            fleet.machine(2).unwrap().core_update_policy,
            CoreUpdatePolicy::AutoWhenBehind
        );

        fleet
            .set_agent_signed_in(2, MachineAgentKind::ClaudeCode, false)
            .unwrap();
        assert_eq!(
            fleet.machine(2).unwrap().claude_status_summary(),
            "/usr/local/bin/claude · isolated config · not signed in"
        );

        assert_eq!(
            fleet.set_max_sessions_at_once(2, 0),
            Err(MachinesError::InvalidSessionLimit)
        );
        fleet.set_max_sessions_at_once(2, 6).unwrap();
        assert_eq!(fleet.machine(2).unwrap().max_sessions_at_once, 6);

        let check_msg = fleet.check_core_now(2).unwrap();
        assert!(check_msg.contains("matches data schema 7"));

        let conn_msg = fleet.test_connection(2).unwrap();
        assert!(conn_msg.contains("builder@10.0.4.21"));
    }
}
