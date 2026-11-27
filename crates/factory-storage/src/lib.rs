//! SQLite persistence (`bruk.db`), plaintext-to-keychain secret migration, and
//! dashboard analytics backed by persisted runs and chat usage.

pub mod dashboard;
pub mod secrets_migration;
pub mod storage;

pub use dashboard::{
    DashboardDataset, DashboardGranularity, DashboardInsight, DashboardQuery, DashboardSnapshot,
    HighTokenRun, InboxItem, InboxRequestKind, MachineNode, MachineStatus, ProjectActivityRow,
    ProjectColor, ProjectTokens, ProviderUsage, TokenBucket, WaitMetrics, format_axis_tokens,
    format_currency, format_tenths_hours, format_tokens, format_wait_minutes,
};
pub use secrets_migration::{
    LEGACY_DATABASE_SIDE_EXTENSIONS, MigrationReport, PathChangeResolution,
    delete_legacy_database_files, find_legacy_database_files, migrate_plaintext_secrets,
};
pub use storage::{PersistedChatUsage, PersistedWorkflowRun, ProjectDatabase, StorageError};
