//! Machines fleet domain model, schema compatibility, catch-up summaries, and active sessions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current Wald desktop application version displayed in the Machines header.
pub const WALD_APP_VERSION: &str = "0.9.0";

/// Current Wald data schema version compared against each remote core on connection.
pub const WALD_DATA_SCHEMA: u32 = 7;

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

/// Live SSH and sync state for a machine running a Wald core.
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

/// Result of comparing a machine core's data schema against Wald's current schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaCompatibility {
    InSync,
    CoreUpdateSuggested,
    NeedsNewerWald,
}

impl SchemaCompatibility {
    pub fn from_schemas(wald_schema: u32, core_schema: u32) -> Self {
        if core_schema == wald_schema {
            Self::InSync
        } else if core_schema < wald_schema {
            Self::CoreUpdateSuggested
        } else {
            Self::NeedsNewerWald
        }
    }

    pub fn badge_label(self) -> Option<&'static str> {
        match self {
            Self::InSync => None,
            Self::CoreUpdateSuggested => Some("Core update suggested"),
            Self::NeedsNewerWald => Some("Needs a newer Wald"),
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

/// Full local or remote machine record displayed in the Machines master-detail view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRecord {
    pub id: u64,
    pub name: String,
    pub platform: MachinePlatform,
    pub os_short: String,
    pub os_detail: String,
    pub hardware_spec: String,
    pub is_local: bool,
    pub status: MachineConnectionStatus,
    pub status_detail: String,
    pub address_primary: String,
    pub address_secondary: String,
    pub ssh_target: String,
    pub ssh_key_fingerprint: String,
    pub core_version: String,
    pub core_schema: u32,
    pub core_sync_note: String,
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

    pub fn compatibility(&self, wald_schema: u32) -> SchemaCompatibility {
        SchemaCompatibility::from_schemas(wald_schema, self.core_schema)
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
    #[error("machine '{0}' already exists")]
    DuplicateName(String),
    #[error("machine #{0} was not found")]
    NotFound(u64),
    #[error("the local machine cannot be disconnected or removed")]
    LocalMachineProtected,
}

/// Fleet of local and remote machines managed by Wald.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachinesFleet {
    pub wald_version: String,
    pub wald_schema: u32,
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
            wald_version: WALD_APP_VERSION.to_owned(),
            wald_schema: WALD_DATA_SCHEMA,
            machines: vec![
                MachineRecord {
                    id: 1,
                    name: "This Mac".to_owned(),
                    platform: MachinePlatform::Mac,
                    os_short: "macOS 26.1 · arm64".to_owned(),
                    os_detail: "macOS 26.1".to_owned(),
                    hardware_spec: "arm64 · 16 cores · 64 GB".to_owned(),
                    is_local: true,
                    status: MachineConnectionStatus::Connected,
                    status_detail: "Local".to_owned(),
                    address_primary: "localhost".to_owned(),
                    address_secondary: "Unix domain socket".to_owned(),
                    ssh_target: "Local transport".to_owned(),
                    ssh_key_fingerprint: "Direct IPC · no SSH key".to_owned(),
                    core_version: "0.9.0".to_owned(),
                    core_schema: 7,
                    core_sync_note: "Bundled · in sync".to_owned(),
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
                    os_detail: "Ubuntu 24.04.1 LTS".to_owned(),
                    hardware_spec: "x86_64 · 16 cores · 64 GB".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::Connected,
                    status_detail: "Caught up 2 min ago".to_owned(),
                    address_primary: "10.0.4.21:22".to_owned(),
                    address_secondary: "fd00::4:21".to_owned(),
                    ssh_target: "wald@build-box-01".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:q3Ff…8k…".to_owned(),
                    core_version: "0.8.5".to_owned(),
                    core_schema: 7,
                    core_sync_note: "Same schema · in sync".to_owned(),
                    sessions_subtitle: "Live again after catching up".to_owned(),
                    catch_up: Some(MachineCatchUpSummary {
                        headline: "Reconnected 2 minutes ago and caught up".to_owned(),
                        summary: "While Wald was away, the 4 sessions here produced 318 events: 3 steps finished, one loop ran out of attempts, and 2 new requests are in the Inbox.".to_owned(),
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
                    os_detail: "Windows 11 24H2".to_owned(),
                    hardware_spec: "x86_64 · 12 cores · 32 GB".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::Disconnected,
                    status_detail: "Last seen 3 h ago".to_owned(),
                    address_primary: "10.0.4.29:22".to_owned(),
                    address_secondary: "fd00::4:29".to_owned(),
                    ssh_target: "wald@win-ci-02".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:k8Lm…2p…".to_owned(),
                    core_version: "0.8.3".to_owned(),
                    core_schema: 6,
                    core_sync_note: "Older schema · update suggested".to_owned(),
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
                    os_detail: "Fedora 41 Workstation".to_owned(),
                    hardware_spec: "x86_64 · 32 cores · 128 GB".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::NotSyncing,
                    status_detail: "Connected 5 min ago".to_owned(),
                    address_primary: "10.0.4.44:22".to_owned(),
                    address_secondary: "fd00::4:44".to_owned(),
                    ssh_target: "wald@gpu-01".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:x9Rt…4n…".to_owned(),
                    core_version: "1.0.0".to_owned(),
                    core_schema: 8,
                    core_sync_note: "Newer schema · not syncing".to_owned(),
                    sessions_subtitle: "Paused sync until Wald is updated".to_owned(),
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
                    os_detail: "macOS 15.6".to_owned(),
                    hardware_spec: "arm64 · 10 cores · 24 GB".to_owned(),
                    is_local: false,
                    status: MachineConnectionStatus::Unreachable,
                    status_detail: "Since Sunday".to_owned(),
                    address_primary: "10.0.4.38:22".to_owned(),
                    address_secondary: "fd00::4:38".to_owned(),
                    ssh_target: "wald@mini-lab".to_owned(),
                    ssh_key_fingerprint: "ED25519 · SHA256:m2Vx…9q…".to_owned(),
                    core_version: "0.9.0".to_owned(),
                    core_schema: 7,
                    core_sync_note: "Same schema · offline".to_owned(),
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
                m.compatibility(self.wald_schema) == SchemaCompatibility::CoreUpdateSuggested
            })
            .count()
    }

    pub fn needs_newer_wald_count(&self) -> usize {
        self.machines
            .iter()
            .filter(|m| m.compatibility(self.wald_schema) == SchemaCompatibility::NeedsNewerWald)
            .count()
    }

    /// Formats the unified top-bar subtitle:
    /// `5 machines · 10 sessions · Wald 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Wald`
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
                "Wald {} (data schema {})",
                self.wald_version, self.wald_schema
            ),
        ];

        let updates = self.core_update_suggested_count();
        if updates == 1 {
            parts.push("1 core update suggested".to_owned());
        } else if updates > 1 {
            parts.push(format!("{updates} core updates suggested"));
        }

        let newer = self.needs_newer_wald_count();
        if newer == 1 {
            parts.push("1 machine needs a newer Wald".to_owned());
        } else if newer > 1 {
            parts.push(format!("{newer} machines need a newer Wald"));
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
        let wald_schema = self.wald_schema;
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
        machine.core_sync_note = match machine.compatibility(wald_schema) {
            SchemaCompatibility::InSync => "Same schema · offline".to_owned(),
            SchemaCompatibility::CoreUpdateSuggested => {
                "Older schema · update suggested".to_owned()
            }
            SchemaCompatibility::NeedsNewerWald => "Newer schema · not syncing".to_owned(),
        };
        Ok(&self.machines[idx])
    }

    pub fn reconnect_machine(&mut self, id: u64) -> Result<&MachineRecord, MachinesError> {
        let idx = self
            .machines
            .iter()
            .position(|m| m.id == id)
            .ok_or(MachinesError::NotFound(id))?;
        let wald_schema = self.wald_schema;
        let machine = &mut self.machines[idx];
        let compat = machine.compatibility(wald_schema);
        match compat {
            SchemaCompatibility::NeedsNewerWald => {
                machine.status = MachineConnectionStatus::NotSyncing;
                machine.status_detail = "Connected just now".to_owned();
                machine.core_sync_note = "Newer schema · not syncing".to_owned();
                machine.sessions_subtitle = "Paused sync until Wald is updated".to_owned();
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
        let target_version = self.wald_version.clone();
        let target_schema = self.wald_schema;
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

    /// Updates every machine whose core schema is older than Wald's current schema.
    pub fn update_all_suggested_cores(&mut self) -> usize {
        let wald_version = self.wald_version.clone();
        let wald_schema = self.wald_schema;
        let mut updated = 0;
        for machine in &mut self.machines {
            if machine.compatibility(wald_schema) == SchemaCompatibility::CoreUpdateSuggested {
                machine.core_version = wald_version.clone();
                machine.core_schema = wald_schema;
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

        let record = MachineRecord {
            id: next_id,
            name,
            platform: draft.platform,
            os_short: format!("{os_label} · {arch}"),
            os_detail: os_label,
            hardware_spec: format!("{arch} · 8 cores · 32 GB"),
            is_local: false,
            status: MachineConnectionStatus::Connected,
            status_detail: "Connected just now".to_owned(),
            address_primary,
            address_secondary: format!("fd00::4:{}", 50 + next_id),
            ssh_target,
            ssh_key_fingerprint: format!("ED25519 · SHA256:n{next_id}Wk…7c…"),
            core_version: self.wald_version.clone(),
            core_schema: self.wald_schema,
            core_sync_note: "Same schema · in sync".to_owned(),
            sessions_subtitle: "Ready for new sessions".to_owned(),
            catch_up: None,
            sessions: Vec::new(),
            allowed_projects,
        };

        self.machines.push(record);
        Ok(self.machines.last().expect("machine was just pushed"))
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
        assert_eq!(fleet.needs_newer_wald_count(), 1);
        assert_eq!(
            fleet.header_subtitle(),
            "5 machines · 10 sessions · Wald 0.9.0 (data schema 7) · 1 core update suggested · 1 machine needs a newer Wald"
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
            "5 machines · 10 sessions · Wald 0.9.0 (data schema 7) · 1 machine needs a newer Wald"
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
                ssh_target: "wald@ci-arm-03".to_owned(),
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
}
