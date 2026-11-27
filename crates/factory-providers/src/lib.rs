//! Provider CLI discovery, authentication probing, usage/model parsing, and spawn settings
//! for Claude, Codex, and Antigravity.

pub mod antigravity_cli;
pub mod claude_cli;
pub mod claude_settings;
pub mod codex_cli;
pub mod codex_settings;

pub use antigravity_cli::{
    ANTIGRAVITY_BINARY_NAME, ANTIGRAVITY_DEFAULT_CONFIG_DIR, ANTIGRAVITY_DEFAULT_MODEL,
    ANTIGRAVITY_DISABLE_AUTO_UPDATE_ENV, ANTIGRAVITY_DISABLE_AUTO_UPDATE_VALUE,
    AntigravityBinaryError, AntigravityCliDetails, AntigravityCliProbe, AntigravityContextReading,
    antigravity_tool_label, antigravity_usage_tokens, apply_mandatory_antigravity_spawn_env,
    fetch_antigravity_details, fetch_antigravity_usage, new_antigravity_command,
    parse_antigravity_models_output, parse_antigravity_usage_output, probe_antigravity_cli,
    probe_antigravity_cli_from_env, read_antigravity_context, resolve_antigravity_binary,
    resolve_antigravity_binary_from_env, run_antigravity_login,
};
pub use claude_cli::{
    CLAUDE_BINARY_NAME, ClaudeBinaryError, ClaudeCliDetails, ClaudeCliProbe, ClaudeModelOption,
    ClaudeQuotaReset, ClaudeUsageSummary, ClaudeUsageWindow, expand_tilde, fetch_claude_details,
    format_reset_countdown, format_reset_countdown_now, format_reset_schedule,
    format_reset_schedule_now, model_family_from_id, parse_claude_auth_status,
    parse_claude_details_stream, probe_claude_cli, probe_claude_cli_from_env,
    resolve_claude_binary, resolve_claude_binary_from_env, run_claude_auth_login,
};
pub use claude_settings::{
    CLAUDE_DISABLE_AUTOUPDATER_ENV, CLAUDE_DISABLE_AUTOUPDATER_VALUE, CLAUDE_ENV_PRESETS,
    CLAUDE_SETTING_SPECS, ClaudeEnvPreset, ClaudeSettingCategory, ClaudeSettingKind,
    ClaudeSettingSpec, ClaudeSettingTarget, ClaudeSettingValue, ClaudeSettingsError,
    ClaudeSpawnOptions, ClaudeSpawnPlan, ClaudeSpawnSettings, CliFlagEncoding,
    apply_mandatory_claude_spawn_env, build_claude_command, build_claude_spawn_plan,
    claude_setting_specs_for_category, find_claude_setting_spec, new_claude_command,
    split_shell_words,
};
pub use codex_cli::{
    CODEX_BINARY_NAME, CodexBinaryError, CodexCliDetails, CodexCliProbe, fetch_codex_details,
    parse_codex_account_stream, parse_codex_details_stream, probe_codex_cli,
    probe_codex_cli_from_env, resolve_codex_binary, resolve_codex_binary_from_env, run_codex_login,
};
pub use codex_settings::{
    CODEX_DISABLE_UPDATE_CHECK_ARG, CODEX_DISABLE_UPDATE_CHECK_KEY,
    CODEX_DISABLE_UPDATE_CHECK_VALUE, CODEX_ENV_PRESETS, CODEX_SETTING_SPECS, CodexEnvPreset,
    CodexSettingCategory, CodexSettingKind, CodexSettingSpec, CodexSettingTarget,
    CodexSettingValue, CodexSettingsError, CodexSpawnOptions, CodexSpawnPlan, CodexSpawnSettings,
    apply_mandatory_codex_spawn_args, build_codex_command, build_codex_spawn_plan,
    codex_setting_specs_for_category, find_codex_setting_spec, new_codex_subcommand,
};
