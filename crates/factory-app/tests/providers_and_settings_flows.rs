//! Headless Native Shell UI tests for Claude/Codex/Antigravity providers, fonts, MCP, and secrets.

mod common;

use std::ffi::OsString;

use tempfile::TempDir;

#[allow(unused_imports)]
use common::*;

#[cfg(unix)]
#[test]
fn claude_cli_on_path_and_logged_in_hides_sign_in_and_uses_system_account() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    let claude_path = write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"system-user@example.com","subscriptionType":"max","planTier":"max_5x","configDirectory":"/Users/system/.claude"}'
"#,
    );

    let mut app = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());

    // Open Preferences (⌘,); Claude is selected by default when no OpenRouter keys exist.
    app.press_key("mod+,").unwrap();
    app.wait_for("system-user@example.com").unwrap();

    assert!(app.screen().shows("CLI works"));
    assert!(app.screen().shows("Max 5x"));
    assert!(app.screen().shows(&claude_path.display().to_string()));
    assert!(app.screen().shows("/Users/system/.claude"));
    assert!(
        app.screen()
            .shows("Using your existing system Claude CLI login.")
    );
    // Authentication is proven on the system, so "Sign In" must not appear.
    assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());
    assert!(!app.screen().matches("role:button[Sign In]").unwrap());
}

#[cfg(unix)]
#[test]
fn claude_cli_missing_from_path_accepts_custom_path_and_persists_across_restart() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let custom_dir = temp.path().join("opt-claude");
    std::fs::create_dir_all(&custom_dir).unwrap();

    let custom_claude = write_fake_claude(
        &custom_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"custom-path@example.com","subscriptionType":"pro","configDirectory":"/Users/custom/.claude"}'
"#,
    );
    let custom_claude_str = custom_claude.display().to_string();

    {
        let mut app = start_with_db_and_claude_path(db_path.clone(), OsString::new());
        app.press_key("mod+,").unwrap();
        app.wait_for("Not found on default PATH").unwrap();
        assert!(app.screen().shows("CLI not found on PATH"));
        // Sign In is not shown when the binary itself is missing.
        assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());

        // Open the path editor and verify invalid paths report an error.
        app.press("id:prefs-claude-path-edit").unwrap();
        app.wait_for("id:prefs-claude-path-input").unwrap();
        app.type_text("id:prefs-claude-path-input", "/nonexistent/claude")
            .unwrap();
        app.press("id:prefs-claude-path-save").unwrap();
        app.wait_for("Claude CLI was not found at '/nonexistent/claude'.")
            .unwrap();

        // Enter the valid custom binary path; Bruk probes and proves system authentication.
        app.clear("id:prefs-claude-path-input").unwrap();
        app.type_text("id:prefs-claude-path-input", &custom_claude_str)
            .unwrap();
        app.press("id:prefs-claude-path-save").unwrap();
        app.wait_for("custom-path@example.com").unwrap();
        assert!(app.screen().shows("CLI works"));
        assert!(app.screen().shows("Pro"));
        assert!(app.screen().shows("Custom"));
        assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());
        assert!(!app.screen().matches("role:button[Sign In]").unwrap());

        app.press_window_close("preferences").unwrap();
    }

    // Reopen with an empty PATH and verify the custom path persisted and stays authenticated.
    let mut reopened = start_with_db_and_claude_path(db_path, OsString::new());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("custom-path@example.com").unwrap();
    assert!(reopened.screen().shows("CLI works"));
    assert!(reopened.screen().shows(&custom_claude_str));
    assert!(
        !reopened
            .screen()
            .matches("id:prefs-claude-sign-in")
            .unwrap()
    );

    // Clearing back to default PATH returns to the not-found state on the empty PATH.
    reopened.press("id:prefs-claude-path-edit").unwrap();
    reopened.wait_for("id:prefs-claude-path-reset").unwrap();
    reopened.press("id:prefs-claude-path-reset").unwrap();
    reopened.wait_for("Not found on default PATH").unwrap();
}

#[cfg(unix)]
#[test]
fn claude_cli_on_path_but_logged_out_shows_sign_in_and_updates_after_login() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    let state_file = temp.path().join("logged_in");
    std::fs::create_dir_all(&bin_dir).unwrap();

    let script = format!(
        r#"#!/bin/sh
STATE_FILE="{state}"
if [ "$1" = "auth" ] && [ "$2" = "login" ]; then
  touch "$STATE_FILE"
  exit 0
fi
if [ -f "$STATE_FILE" ]; then
  echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"fresh-login@example.com","subscriptionType":"max","configDirectory":"/Users/fresh/.claude"}}'
  exit 0
else
  echo '{{"loggedIn":false,"authMethod":"none","configDirectory":"/Users/fresh/.claude"}}'
  exit 1
fi
"#,
        state = state_file.display()
    );
    write_fake_claude(&bin_dir, "claude", &script);

    let mut app = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());
    app.press_key("mod+,").unwrap();
    app.wait_for("id:prefs-claude-sign-in").unwrap();
    assert!(app.screen().shows("Not signed in"));

    // Pressing Sign In runs `claude auth login` and refreshes into the authenticated state,
    // hiding the Sign In button automatically.
    app.press("id:prefs-claude-sign-in").unwrap();
    app.wait_for("fresh-login@example.com").unwrap();
    assert!(app.screen().shows("CLI works"));
    assert!(!app.screen().matches("id:prefs-claude-sign-in").unwrap());
    assert!(!app.screen().matches("role:button[Sign In]").unwrap());
}

#[cfg(unix)]
#[test]
fn claude_cli_queries_available_models_and_remaining_usage_for_ui() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    let usage_counter = temp.path().join("usage_count");
    std::fs::create_dir_all(&bin_dir).unwrap();

    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"live-cli@example.com","subscriptionType":"max","planTier":"max_5x","configDirectory":"/Users/live/.claude"}}'
  exit 0
fi

# Consume stdin control requests and emit initialize + get_usage control responses.
cat >/dev/null
COUNT_FILE="{counter}"
if [ -f "$COUNT_FILE" ]; then
  SESSION_PCT=22
else
  touch "$COUNT_FILE"
  SESSION_PCT=8
fi
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"bruk_init","response":{{"models":[{{"value":"default","resolvedModel":"claude-opus-5-5","displayName":"Default (recommended)","description":"Opus 5.5"}},{{"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus 5.5","description":"For complex work"}},{{"value":"fable","resolvedModel":"claude-fable-5-2","displayName":"Fable 5.2","description":"For your toughest challenges"}},{{"value":"sonnet","resolvedModel":"claude-sonnet-5-5","displayName":"Sonnet 5.5","description":"Most efficient"}}]}}}}}}'
echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"bruk_usage","response":{{"rate_limits_available":true,"rate_limits":{{"limits":[{{"kind":"session","group":"session","percent":'"$SESSION_PCT"',"resets_at":"2099-11-22T23:59:59+00:00","severity":"normal","scope":null}},{{"kind":"weekly_all","group":"weekly","percent":12,"resets_at":"2099-11-29T20:00:00+00:00","severity":"normal","scope":null}},{{"kind":"weekly_scoped","group":"weekly","percent":5,"resets_at":"2099-11-29T20:00:00+00:00","severity":"normal","scope":{{"model":{{"display_name":"Fable"}}}}}}]}}}}}}}}'
"#,
        counter = usage_counter.display()
    );
    write_fake_claude(&bin_dir, "claude", &script);

    let mut app =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    app.press_key("mod+,").unwrap();

    // Remaining usage windows and scheduled quota resets queried from the CLI appear in the card.
    app.wait_for("92% remaining").unwrap();
    assert!(app.screen().shows("Usage remaining"));
    assert!(app.screen().shows("Quota resets"));
    assert!(app.screen().shows("Current session (5h)"));
    assert!(app.screen().shows("Weekly (all models)"));
    assert!(app.screen().shows("88% remaining"));
    assert!(app.screen().shows("Weekly (Fable)"));
    assert!(app.screen().shows("95% remaining"));

    // Refresh re-queries the CLI and updates remaining usage live.
    app.press("id:prefs-claude-usage-refresh").unwrap();
    app.wait_for("78% remaining").unwrap();

    // Opening the Model editor shows the live models queried from the CLI.
    app.press("id:prefs-cli-model-change").unwrap();
    app.wait_for("Available from Claude CLI (3 models)")
        .unwrap();
    assert!(app.screen().shows("Model Family Filter"));
    assert!(app.screen().shows("Opus 5.5"));
    assert!(app.screen().shows("Fable 5.2"));
    assert!(app.screen().shows("Sonnet 5.5"));

    // Unchecking Fable in the Model Family Filter hides it from the model selection view.
    app.press("id:prefs-claude-family-fable").unwrap();
    app.wait_for("Showing 2 of 3 models from Claude CLI (Fable hidden)")
        .unwrap();
    assert!(!app.screen().shows("Fable 5.2"));
    assert!(app.screen().shows("Opus 5.5"));
    assert!(app.screen().shows("Sonnet 5.5"));

    // Re-checking Fable restores it immediately.
    app.press("id:prefs-claude-family-fable").unwrap();
    app.wait_for("Available from Claude CLI (3 models)")
        .unwrap();
    assert!(app.screen().shows("Fable 5.2"));

    // Uncheck Fable again and verify the filter persists across app restarts.
    app.press("id:prefs-claude-family-fable").unwrap();
    app.wait_for("Showing 2 of 3 models from Claude CLI (Fable hidden)")
        .unwrap();

    app.press("id:prefs-claude-model-sonnet").unwrap();
    app.press("role:button[Save Model]").unwrap();
    app.wait_for("Default model set to sonnet.").unwrap();
    assert!(app.screen().shows("sonnet"));
    app.press_window_close("preferences").unwrap();

    // Reopen against the same SQLite DB and verify Fable stays hidden.
    let mut reopened = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("Usage remaining").unwrap();
    reopened.press("id:prefs-cli-model-change").unwrap();
    reopened
        .wait_for("Showing 2 of 3 models from Claude CLI (Fable hidden)")
        .unwrap();
    assert!(!reopened.screen().shows("Fable 5.2"));
    assert!(reopened.screen().shows("Opus 5.5"));
    assert!(reopened.screen().shows("Sonnet 5.5"));
}

#[test]
fn codex_provider_is_unregistered_by_default_and_can_be_added_and_removed() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    {
        let mut app = start_with_db(db_path.clone());

        // Register a project so we can also check Project Settings › Providers.
        app.press("id:dashboard-empty-register-project").unwrap();
        app.wait_for("Add a project").unwrap();
        app.press("id:register-mode-link").unwrap();
        app.type_text("text:Project name", "my-service").unwrap();
        app.type_text("text:Repository path", "/workspace/my-service")
            .unwrap();
        app.press("role:button[Link Project]").unwrap();
        app.wait_for("Registered project 'my-service'").unwrap();

        // Project Settings › Providers must not show Codex as if it were registered.
        app.press_key("mod+shift+,").unwrap();
        app.press("id:ps-tab-providers").unwrap();
        app.wait_for("Available accounts").unwrap();
        assert!(!app.screen().shows("Codex CLI"));
        assert!(!app.screen().matches("role:button[Codex]").unwrap());

        // Preferences › Providers shows Codex as unconfigured, not as a registered account.
        app.press_key("mod+,").unwrap();
        app.wait_for("No Codex account configured").unwrap();
        assert!(!app.screen().matches("role:button[Codex]").unwrap());

        // Register Codex explicitly through Add Account.
        app.press("role:button[Add Account]").unwrap();
        app.wait_for("Add Codex CLI account...").unwrap();
        app.press("role:button[Add Codex CLI account...]").unwrap();
        app.wait_for("Add Codex CLI account").unwrap();
        app.press("id:prefs-new-codex-submit").unwrap();
        app.wait_for("Added Codex CLI account.").unwrap();
        assert!(app.screen().matches("role:button[Codex]").unwrap());
        assert!(app.screen().shows("gpt-5.3-codex"));
        app.press_window_close("preferences").unwrap();

        // Once registered, Codex appears in Project Settings › Providers.
        app.wait_for("Codex CLI").unwrap();
        assert!(app.screen().matches("role:button[Codex]").unwrap());
    }

    // Reopen and verify the registered Codex account persisted, then remove it.
    {
        let mut reopened = start_with_db(db_path.clone());
        reopened.press_key("mod+,").unwrap();
        select_provider(&mut reopened, "role:button[Codex]", "id:prefs-codex-remove");
        reopened.press("id:prefs-codex-remove").unwrap();
        reopened.wait_for("id:prefs-codex-confirm-remove").unwrap();
        reopened.press("id:prefs-codex-confirm-remove").unwrap();
        reopened.wait_for("No Codex account configured").unwrap();
        assert!(!reopened.screen().matches("role:button[Codex]").unwrap());
        reopened.press_window_close("preferences").unwrap();
    }

    // Reopen again and confirm Codex remains unregistered.
    let mut after_remove = start_with_db(db_path);
    after_remove.press_key("mod+,").unwrap();
    after_remove
        .wait_for("No Codex account configured")
        .unwrap();
    assert!(!after_remove.screen().matches("role:button[Codex]").unwrap());
}

#[cfg(unix)]
#[test]
fn codex_cli_queries_available_models_usage_resets_and_persists_family_filter() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    let usage_counter = temp.path().join("codex_usage_count");
    std::fs::create_dir_all(&bin_dir).unwrap();

    // Put a minimal fake `claude` alongside `codex` so Claude stays deterministic on this PATH.
    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":false,"authMethod":"none","configDirectory":"/tmp/.claude"}'
exit 1
"#,
    );

    let codex_script = format!(
        r#"#!/bin/sh
COUNT_FILE="{counter}"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      echo '{{"id":1,"result":{{"userAgent":"bruk/0.171.0","codexHome":"/Users/live/.codex"}}}}'
      ;;
    *'"method":"account/read"'*)
      echo '{{"id":2,"result":{{"account":{{"type":"chatgpt","email":"codex-live@example.com","planType":"prolite","workspaceName":null}},"requiresOpenaiAuth":true}}}}'
      ;;
    *'"method":"account/rateLimits/read"'*)
      if [ -f "$COUNT_FILE" ]; then
        SESSION_PCT=24
      else
        touch "$COUNT_FILE"
        SESSION_PCT=10
      fi
      echo '{{"id":2,"result":{{"ordinaryUsageAllowed":true,"rateLimits":{{"limitId":"codex","limitName":null,"primary":{{"usedPercent":'"$SESSION_PCT"',"windowDurationMins":300,"resetsAt":4099075199}},"secondary":{{"usedPercent":15,"windowDurationMins":10080,"resetsAt":4099665600}}}},"rateLimitsByLimitId":{{"codex":{{"limitId":"codex","limitName":null,"primary":{{"usedPercent":'"$SESSION_PCT"',"windowDurationMins":300,"resetsAt":4099075199}},"secondary":{{"usedPercent":15,"windowDurationMins":10080,"resetsAt":4099665600}}}},"codex_spark":{{"limitId":"codex_spark","limitName":"GPT-5.3-Codex-Spark","primary":{{"usedPercent":5,"windowDurationMins":10080,"resetsAt":4099665600}}}}}},"rateLimitResetCredits":{{"availableCount":2}}}}}}'
      ;;
    *'"method":"model/list"'*)
      echo '{{"id":3,"result":{{"data":[{{"id":"gpt-6.2-sol","model":"gpt-6.2-sol","displayName":"GPT-6.2-Sol","description":"Fast go-to model","hidden":false}},{{"id":"gpt-6.2-astra","model":"gpt-6.2-astra","displayName":"GPT-6.2-Astra","description":"Most capable model","hidden":false}},{{"id":"gpt-6-luna","model":"gpt-6-luna","displayName":"GPT-6-Luna","description":"Fast and affordable","hidden":false}},{{"id":"hidden-model","model":"hidden-model","displayName":"Hidden","description":"Skipped","hidden":true}}]}}}}'
      ;;
  esac
done
"#,
        counter = usage_counter.display()
    );
    write_fake_claude(&bin_dir, "codex", &codex_script);

    let mut app =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    app.press_key("mod+,").unwrap();
    app.wait_for("No Codex account configured").unwrap();

    // Register Codex explicitly through Add Account.
    app.press("role:button[Add Account]").unwrap();
    app.wait_for("Add Codex CLI account...").unwrap();
    app.press("role:button[Add Codex CLI account...]").unwrap();
    app.wait_for("Add Codex CLI account").unwrap();
    app.press("id:prefs-new-codex-submit").unwrap();

    // Codex probes system auth and queries live usage, resets, and models.
    app.wait_for("90% remaining").unwrap();
    assert!(app.screen().shows("codex-live@example.com"));
    assert!(app.screen().shows("Pro Lite"));
    assert!(app.screen().shows("/Users/live/.codex"));
    assert!(app.screen().shows("Usage remaining"));
    assert!(app.screen().shows("Quota resets"));
    assert!(app.screen().shows("Current session (5h)"));
    assert!(app.screen().shows("Weekly (all models)"));
    assert!(app.screen().shows("85% remaining"));
    assert!(app.screen().shows("Weekly (GPT-5.3-Codex-Spark)"));
    assert!(app.screen().shows("95% remaining"));
    assert!(app.screen().shows("2 rate limit reset credits available."));
    assert!(!app.screen().matches("id:prefs-codex-sign-in").unwrap());

    // Refresh re-queries the CLI and updates remaining usage live.
    app.press("id:prefs-codex-usage-refresh").unwrap();
    app.wait_for("76% remaining").unwrap();

    // Opening the Model editor shows the live Codex models queried from the CLI.
    app.press("id:prefs-cli-model-change").unwrap();
    app.wait_for("Available from Codex CLI (3 models)").unwrap();
    assert!(app.screen().shows("Model Family Filter"));
    assert!(app.screen().shows("GPT-6.2-Sol"));
    assert!(app.screen().shows("GPT-6.2-Astra"));
    assert!(app.screen().shows("GPT-6-Luna"));
    assert!(!app.screen().shows("Hidden"));

    // Unchecking Luna in the Model Family Filter hides it from the model selection view.
    app.press("id:prefs-codex-family-luna").unwrap();
    app.wait_for("Showing 2 of 3 models from Codex CLI (Luna hidden)")
        .unwrap();
    assert!(!app.screen().shows("GPT-6-Luna"));
    assert!(app.screen().shows("GPT-6.2-Sol"));
    assert!(app.screen().shows("GPT-6.2-Astra"));

    // Re-checking Luna restores it immediately.
    app.press("id:prefs-codex-family-luna").unwrap();
    app.wait_for("Available from Codex CLI (3 models)").unwrap();
    assert!(app.screen().shows("GPT-6-Luna"));

    // Uncheck Luna again, select GPT-6.2-Sol, and verify the filter persists across app restarts.
    app.press("id:prefs-codex-family-luna").unwrap();
    app.wait_for("Showing 2 of 3 models from Codex CLI (Luna hidden)")
        .unwrap();
    app.press("id:prefs-codex-model-gpt-6.2-sol").unwrap();
    app.press("role:button[Save Model]").unwrap();
    app.wait_for("Default model set to gpt-6.2-sol.").unwrap();
    app.press_window_close("preferences").unwrap();

    // Reopen against the same SQLite DB and verify Luna stays hidden and usage reloads.
    let mut reopened = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    select_provider(&mut reopened, "role:button[Codex]", "id:prefs-codex-remove");
    reopened.wait_for("Usage remaining").unwrap();
    reopened.wait_for("76% remaining").unwrap();
    reopened.press("id:prefs-cli-model-change").unwrap();
    reopened
        .wait_for("Showing 2 of 3 models from Codex CLI (Luna hidden)")
        .unwrap();
    assert!(!reopened.screen().shows("GPT-6-Luna"));
    assert!(reopened.screen().shows("GPT-6.2-Sol"));
    assert!(reopened.screen().shows("GPT-6.2-Astra"));
}

#[cfg(unix)]
#[test]
fn codex_cli_custom_path_and_sign_in_flow() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let custom_dir = temp.path().join("opt-codex");
    let state_file = temp.path().join("codex_logged_in");
    std::fs::create_dir_all(&custom_dir).unwrap();

    let codex_script = format!(
        r#"#!/bin/sh
STATE_FILE="{state}"
if [ "$1" = "login" ]; then
  touch "$STATE_FILE"
  exit 0
fi
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      echo '{{"id":1,"result":{{"codexHome":"/Users/custom/.codex"}}}}'
      ;;
    *'"method":"account/read"'*)
      if [ -f "$STATE_FILE" ]; then
        echo '{{"id":2,"result":{{"account":{{"type":"chatgpt","email":"fresh-codex@example.com","planType":"pro"}},"requiresOpenaiAuth":true}}}}'
      else
        echo '{{"id":2,"result":{{"account":null,"requiresOpenaiAuth":true}}}}'
      fi
      ;;
    *'"method":"account/rateLimits/read"'*)
      echo '{{"id":2,"result":{{"ordinaryUsageAllowed":true,"rateLimits":{{"limitId":"codex","primary":{{"usedPercent":12,"windowDurationMins":10080,"resetsAt":4099665600}}}}}}}}'
      ;;
    *'"method":"model/list"'*)
      echo '{{"id":3,"result":{{"data":[{{"id":"gpt-6.2-sol","model":"gpt-6.2-sol","displayName":"GPT-6.2-Sol","description":"Fast go-to","hidden":false}}]}}}}'
      ;;
  esac
done
"#,
        state = state_file.display()
    );
    let custom_codex = write_fake_claude(&custom_dir, "codex", &codex_script);
    let custom_codex_str = custom_codex.display().to_string();

    let mut app = start_with_db_and_claude_path(db_path, OsString::new());
    app.press_key("mod+,").unwrap();
    app.wait_for("No Codex account configured").unwrap();

    // Register Codex when not on PATH -> shows CLI not found and Set Codex Path CTA.
    app.press("role:button[Add Account]").unwrap();
    app.wait_for("Add Codex CLI account...").unwrap();
    app.press("role:button[Add Codex CLI account...]").unwrap();
    app.wait_for("Add Codex CLI account").unwrap();
    app.press("id:prefs-new-codex-submit").unwrap();
    app.wait_for("Not found on default PATH").unwrap();
    assert!(!app.screen().matches("id:prefs-codex-sign-in").unwrap());

    // Set the custom Codex binary path; it is found but logged out, so Sign In appears.
    app.press("id:prefs-codex-path-edit").unwrap();
    app.wait_for("id:prefs-codex-path-input").unwrap();
    app.type_text("id:prefs-codex-path-input", &custom_codex_str)
        .unwrap();
    app.press("id:prefs-codex-path-save").unwrap();
    app.wait_for("id:prefs-codex-sign-in").unwrap();
    assert!(app.screen().shows("Not signed in"));

    // Pressing Sign In runs `codex login` and refreshes into authenticated + live usage state.
    app.press("id:prefs-codex-sign-in").unwrap();
    app.wait_for("fresh-codex@example.com").unwrap();
    app.wait_for("88% remaining").unwrap();
    assert!(app.screen().shows("CLI works"));
    assert!(!app.screen().matches("id:prefs-codex-sign-in").unwrap());
}

#[cfg(unix)]
#[test]
fn claude_spawn_settings_in_preferences_configure_preview_filter_persist_and_reset() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":true,"authMethod":"claude.ai","email":"spawn-user@example.com","subscriptionType":"max","configDirectory":"/Users/spawn/.claude"}'
"#,
    );

    {
        let mut app =
            start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
        app.press_key("mod+,").unwrap();
        app.wait_for("Spawn Settings").unwrap();
        assert!(app.screen().shows("All defaults"));

        // Collapsible <details>-style sections start collapsed so all 10 groups are scannable.
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Security"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Bare Minimal Mode"));

        // 1. Expand "Launch & CLI" collapsible section and configure CLI flags.
        app.press("id:prefs-claude-spawn-section-cli-flags")
            .unwrap();
        app.wait_for("Bare Minimal Mode").unwrap();

        app.press("id:prefs-claude-spawn-cli-permission-mode-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-cli-permission-mode-opt-acceptedits")
            .unwrap();
        app.press("id:prefs-claude-spawn-cli-permission-mode-opt-acceptedits")
            .unwrap();
        app.wait_for("1 override").unwrap();

        app.press("id:prefs-claude-spawn-cli-bare-on").unwrap();
        app.wait_for("2 overrides").unwrap();

        app.press("id:prefs-claude-spawn-cli-max-budget-usd-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-cli-max-budget-usd-input")
            .unwrap();
        app.type_text("id:prefs-claude-spawn-cli-max-budget-usd-input", "4.50")
            .unwrap();
        app.press("id:prefs-claude-spawn-cli-max-budget-usd-save")
            .unwrap();
        app.wait_for("3 overrides").unwrap();

        // Collapse Launch & CLI again and confirm its rows hide.
        app.press("id:prefs-claude-spawn-section-cli-flags")
            .unwrap();
        assert!(!app.screen().shows("Bare Minimal Mode"));

        // 2. Expand Models & Reasoning and toggle extended thinking.
        app.press("id:prefs-claude-spawn-section-models").unwrap();
        app.wait_for("Always Thinking Enabled").unwrap();
        app.press("id:prefs-claude-spawn-alwaysthinkingenabled-on")
            .unwrap();
        app.wait_for("4 overrides").unwrap();

        // 3. Expand Sandbox & Security and enable sandbox + set proxy port (with validation).
        app.press("id:prefs-claude-spawn-section-sandbox").unwrap();
        app.wait_for("Enable Bash Sandbox").unwrap();
        app.press("id:prefs-claude-spawn-sandbox-enabled-on")
            .unwrap();
        app.wait_for("5 overrides").unwrap();

        app.press("id:prefs-claude-spawn-sandbox-network-httpproxyport-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-sandbox-network-httpproxyport-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-sandbox-network-httpproxyport-input",
            "99999",
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-sandbox-network-httpproxyport-save")
            .unwrap();
        app.wait_for("invalid value for 'sandbox.network.httpProxyPort': must be at most 65535")
            .unwrap();

        app.clear("id:prefs-claude-spawn-sandbox-network-httpproxyport-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-sandbox-network-httpproxyport-input",
            "8080",
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-sandbox-network-httpproxyport-save")
            .unwrap();
        app.wait_for("6 overrides").unwrap();

        // 4. Global search hides non-matching groups and non-matching settings, and auto-expands
        // the matching group ("Context & Git") to show only "Git Commit Attribution".
        app.type_text("id:prefs-claude-spawn-search", "attribution.commit")
            .unwrap();
        app.wait_for("Git Commit Attribution").unwrap();
        assert!(app.screen().shows("Context & Git"));
        assert!(!app.screen().shows("Launch & CLI"));
        assert!(!app.screen().shows("Models & Reasoning"));
        assert!(!app.screen().shows("Sandbox & Security"));
        assert!(!app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Auto-Compact Enabled"));

        app.press("id:prefs-claude-spawn-attribution-commit-edit")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-attribution-commit-save")
            .unwrap();
        app.press("id:prefs-claude-spawn-attribution-commit-save")
            .unwrap();
        app.wait_for("7 overrides").unwrap();
        app.press("id:prefs-claude-spawn-clear-filter").unwrap();
        app.wait_for("Launch & CLI").unwrap();

        // 5. Expand Environment & Raw, confirm DISABLE_AUTOUPDATER=1 is always enforced,
        // and add a preset env var + extra CLI args + extra JSON.
        app.press("id:prefs-claude-spawn-section-env-raw").unwrap();
        app.wait_for("Process Environment Variables").unwrap();
        assert!(app.screen().shows("DISABLE_AUTOUPDATER"));
        assert!(app.screen().shows("Always enforced"));
        app.press("id:prefs-claude-spawn-env-preset-disable-telemetry")
            .unwrap();
        app.wait_for("id:prefs-claude-spawn-env-save").unwrap();
        app.press("id:prefs-claude-spawn-env-save").unwrap();
        app.wait_for("8 overrides").unwrap();
        assert!(app.screen().shows("DISABLE_TELEMETRY"));

        app.press("id:prefs-claude-spawn-extra-args-edit").unwrap();
        app.wait_for("id:prefs-claude-spawn-extra-args-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-extra-args-input",
            "--custom-flag value",
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-extra-args-save").unwrap();
        app.wait_for("9 overrides").unwrap();

        app.press("id:prefs-claude-spawn-extra-json-edit").unwrap();
        app.wait_for("id:prefs-claude-spawn-extra-json-input")
            .unwrap();
        app.type_text(
            "id:prefs-claude-spawn-extra-json-input",
            r#"{"futureOption":true}"#,
        )
        .unwrap();
        app.press("id:prefs-claude-spawn-extra-json-save").unwrap();
        app.wait_for("10 overrides").unwrap();

        // 6. Toggle Preview Command and confirm preview sections appear.
        app.press("id:prefs-claude-spawn-preview-toggle").unwrap();
        app.wait_for("Resolved spawn invocation").unwrap();
        assert!(app.screen().shows("Generated --settings JSON"));
        assert!(
            app.screen()
                .matches("id:prefs-claude-spawn-preview-cmd")
                .unwrap()
        );
        assert!(
            app.screen()
                .matches("id:prefs-claude-spawn-preview-json")
                .unwrap()
        );

        // 7. Toggle "Modified only" to inspect only groups & settings with active overrides
        // (hiding unmodified groups like Permissions and Interface & Session).
        app.press("id:prefs-claude-spawn-modified-only").unwrap();
        app.wait_for("Showing 10 active override(s) across 5 group(s)")
            .unwrap();
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Security"));
        assert!(app.screen().shows("Context & Git"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Permissions"));
        assert!(!app.screen().shows("Interface & Session"));
        assert!(!app.screen().shows("Auto-Compact Enabled"));

        app.press_window_close("preferences").unwrap();
    }

    // Verify persisted SQLite preferences build the exact Claude spawn plan.
    {
        let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
        let saved = db.load_preferences().unwrap().unwrap();
        assert_eq!(saved.claude_spawn_settings.override_count(), 10);
        let plan = saved
            .build_claude_spawn_plan(&factory_core::ClaudeSpawnOptions::default())
            .unwrap();
        let preview = plan.preview_command_line();
        assert!(preview.contains("DISABLE_AUTOUPDATER=1"));
        assert!(preview.contains("DISABLE_TELEMETRY=1"));
        assert!(preview.contains("--permission-mode acceptEdits"));
        assert!(preview.contains("--bare"));
        assert!(preview.contains("--max-budget-usd 4.50"));
        assert!(preview.contains("--custom-flag value"));
        let json = plan.settings_json.unwrap();
        assert_eq!(json["alwaysThinkingEnabled"], true);
        assert_eq!(json["sandbox"]["enabled"], true);
        assert_eq!(json["sandbox"]["network"]["httpProxyPort"], 8080);
        assert_eq!(json["attribution"]["commit"], "");
        assert_eq!(json["futureOption"], true);
    }

    // Reopen against the same SQLite DB and verify all 10 overrides persisted, then Reset All.
    let mut reopened =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("10 overrides").unwrap();
    reopened
        .press("id:prefs-claude-spawn-preview-toggle")
        .unwrap();
    reopened.wait_for("Resolved spawn invocation").unwrap();
    assert!(reopened.screen().shows("Generated --settings JSON"));

    reopened.press("id:prefs-claude-spawn-reset-all").unwrap();
    reopened.wait_for("All defaults").unwrap();
    assert!(!reopened.screen().shows("Generated --settings JSON"));

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let after_reset = db.load_preferences().unwrap().unwrap();
    assert!(after_reset.claude_spawn_settings.is_empty());
    let reset_plan = after_reset
        .build_claude_spawn_plan(&factory_core::ClaudeSpawnOptions::default())
        .unwrap();
    assert_eq!(
        reset_plan
            .env
            .get("DISABLE_AUTOUPDATER")
            .map(String::as_str),
        Some("1")
    );
}

#[cfg(unix)]
#[test]
fn codex_spawn_settings_in_preferences_configure_preview_filter_persist_and_reset() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();

    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":false,"authMethod":"none","configDirectory":"/tmp/.claude"}'
exit 1
"#,
    );
    write_fake_claude(
        &bin_dir,
        "codex",
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      echo '{"id":1,"result":{"userAgent":"bruk/0.171.0","codexHome":"/Users/spawn/.codex"}}'
      ;;
    *'"method":"account/read"'*)
      echo '{"id":2,"result":{"account":{"type":"chatgpt","email":"codex-spawn@example.com","planType":"pro"},"requiresOpenaiAuth":true}}'
      ;;
    *'"method":"account/rateLimits/read"'*)
      echo '{"id":2,"result":{"ordinaryUsageAllowed":true,"rateLimits":null}}'
      ;;
    *'"method":"model/list"'*)
      echo '{"id":3,"result":{"data":[]}}'
      ;;
  esac
done
"#,
    );

    {
        let mut app =
            start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
        app.press_key("mod+,").unwrap();
        app.wait_for("No Codex account configured").unwrap();

        // Register Codex so its detail pane (including Spawn Settings) is active.
        app.press("role:button[Add Account]").unwrap();
        app.wait_for("Add Codex CLI account...").unwrap();
        app.press("role:button[Add Codex CLI account...]").unwrap();
        app.wait_for("Add Codex CLI account").unwrap();
        app.press("id:prefs-new-codex-submit").unwrap();

        app.wait_for("codex-spawn@example.com").unwrap();
        app.wait_for("Spawn Settings").unwrap();
        assert!(app.screen().shows("All defaults"));

        // Collapsible <details>-style sections start collapsed so all 11 groups are scannable.
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Shell"));
        assert!(app.screen().shows("Feature Flags"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Managed Git Worktree"));

        // 1. Expand "Launch & CLI" and configure CLI flags.
        app.press("id:prefs-codex-spawn-section-cli-flags").unwrap();
        app.wait_for("Managed Git Worktree").unwrap();

        app.press("id:prefs-codex-spawn-cli-sandbox-workspace-write")
            .unwrap();
        app.wait_for("1 override").unwrap();

        app.press("id:prefs-codex-spawn-cli-worktree-on").unwrap();
        app.wait_for("2 overrides").unwrap();

        app.press("id:prefs-codex-spawn-cli-profile-edit").unwrap();
        app.wait_for("id:prefs-codex-spawn-cli-profile-input")
            .unwrap();
        app.type_text("id:prefs-codex-spawn-cli-profile-input", "fast")
            .unwrap();
        app.press("id:prefs-codex-spawn-cli-profile-save").unwrap();
        app.wait_for("3 overrides").unwrap();

        // Collapse Launch & CLI again and confirm its rows hide.
        app.press("id:prefs-codex-spawn-section-cli-flags").unwrap();
        assert!(!app.screen().shows("Managed Git Worktree"));

        // 2. Expand Models & Reasoning and configure reasoning effort.
        app.press("id:prefs-codex-spawn-section-models").unwrap();
        app.wait_for("Reasoning Effort").unwrap();
        app.press("id:prefs-codex-spawn-model-reasoning-effort-edit")
            .unwrap();
        app.wait_for("id:prefs-codex-spawn-model-reasoning-effort-opt-high")
            .unwrap();
        app.press("id:prefs-codex-spawn-model-reasoning-effort-opt-high")
            .unwrap();
        app.wait_for("4 overrides").unwrap();

        // 3. Expand Sandbox & Shell and enable network access + test validation in MCP port.
        app.press("id:prefs-codex-spawn-section-sandbox").unwrap();
        app.wait_for("Workspace-Write Network Access").unwrap();
        app.press("id:prefs-codex-spawn-sandbox-workspace-write-network-access-on")
            .unwrap();
        app.wait_for("5 overrides").unwrap();

        app.press("id:prefs-codex-spawn-section-mcp-plugins-hooks")
            .unwrap();
        app.wait_for("MCP OAuth Callback Port").unwrap();
        app.press("id:prefs-codex-spawn-mcp-oauth-callback-port-edit")
            .unwrap();
        app.wait_for("id:prefs-codex-spawn-mcp-oauth-callback-port-input")
            .unwrap();
        app.type_text(
            "id:prefs-codex-spawn-mcp-oauth-callback-port-input",
            "99999",
        )
        .unwrap();
        app.press("id:prefs-codex-spawn-mcp-oauth-callback-port-save")
            .unwrap();
        app.wait_for("invalid value for 'mcp_oauth_callback_port': must be at most 65535")
            .unwrap();

        app.clear("id:prefs-codex-spawn-mcp-oauth-callback-port-input")
            .unwrap();
        app.type_text("id:prefs-codex-spawn-mcp-oauth-callback-port-input", "8484")
            .unwrap();
        app.press("id:prefs-codex-spawn-mcp-oauth-callback-port-save")
            .unwrap();
        app.wait_for("6 overrides").unwrap();

        // 4. Global search hides non-matching groups and auto-expands Feature Flags for memories.
        app.type_text("id:prefs-codex-spawn-search", "features.memories")
            .unwrap();
        app.wait_for("Memories Feature").unwrap();
        assert!(app.screen().shows("Feature Flags"));
        assert!(!app.screen().shows("Launch & CLI"));
        assert!(!app.screen().shows("Models & Reasoning"));
        assert!(!app.screen().shows("Sandbox & Shell"));
        assert!(!app.screen().shows("Environment & Raw"));

        app.press("id:prefs-codex-spawn-features-memories-on")
            .unwrap();
        app.wait_for("7 overrides").unwrap();
        app.press("id:prefs-codex-spawn-clear-filter").unwrap();
        app.wait_for("Launch & CLI").unwrap();

        // 5. Expand Environment & Raw, confirm check_for_update_on_startup=false is always enforced,
        // and add a preset env var + extra CLI args + extra -c config override.
        app.press("id:prefs-codex-spawn-section-env-raw").unwrap();
        app.wait_for("Process Environment Variables").unwrap();
        assert!(app.screen().shows("check_for_update_on_startup"));
        assert!(app.screen().shows("Always enforced"));
        app.press("id:prefs-codex-spawn-env-preset-no-color")
            .unwrap();
        app.wait_for("id:prefs-codex-spawn-env-save").unwrap();
        app.press("id:prefs-codex-spawn-env-save").unwrap();
        app.wait_for("8 overrides").unwrap();
        assert!(app.screen().shows("NO_COLOR"));

        app.press("id:prefs-codex-spawn-extra-args-edit").unwrap();
        app.wait_for("id:prefs-codex-spawn-extra-args-input")
            .unwrap();
        app.type_text("id:prefs-codex-spawn-extra-args-input", "--ephemeral")
            .unwrap();
        app.press("id:prefs-codex-spawn-extra-args-save").unwrap();
        app.wait_for("9 overrides").unwrap();

        app.press("id:prefs-codex-spawn-extra-config-edit").unwrap();
        app.wait_for("id:prefs-codex-spawn-extra-config-input")
            .unwrap();
        app.type_text(
            "id:prefs-codex-spawn-extra-config-input",
            "future_option = true",
        )
        .unwrap();
        app.press("id:prefs-codex-spawn-extra-config-save").unwrap();
        app.wait_for("10 overrides").unwrap();

        // 6. Toggle Preview Command and confirm preview sections appear.
        app.press("id:prefs-codex-spawn-preview-toggle").unwrap();
        app.wait_for("Resolved spawn invocation").unwrap();
        assert!(app.screen().shows("Generated -c config.toml overrides"));
        assert!(
            app.screen()
                .matches("id:prefs-codex-spawn-preview-cmd")
                .unwrap()
        );
        assert!(
            app.screen()
                .matches("id:prefs-codex-spawn-preview-config")
                .unwrap()
        );

        // 7. Toggle "Modified only" to inspect only groups & settings with active overrides.
        app.press("id:prefs-codex-spawn-modified-only").unwrap();
        app.wait_for("Showing 10 active override(s) across 6 group(s)")
            .unwrap();
        assert!(app.screen().shows("Launch & CLI"));
        assert!(app.screen().shows("Models & Reasoning"));
        assert!(app.screen().shows("Sandbox & Shell"));
        assert!(app.screen().shows("MCP, Plugins & Hooks"));
        assert!(app.screen().shows("Feature Flags"));
        assert!(app.screen().shows("Environment & Raw"));
        assert!(!app.screen().shows("Interface & TUI"));
        assert!(!app.screen().shows("Approvals & Permissions"));

        app.press_window_close("preferences").unwrap();
    }

    // Verify persisted SQLite preferences build the exact Codex spawn plan.
    {
        let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
        let saved = db.load_preferences().unwrap().unwrap();
        assert_eq!(saved.codex_spawn_settings.override_count(), 10);
        let plan = saved
            .build_codex_spawn_plan(&factory_core::CodexSpawnOptions::default())
            .unwrap();
        let preview = plan.preview_command_line();
        assert!(preview.contains("NO_COLOR=1"));
        assert!(preview.contains("-c check_for_update_on_startup=false"));
        assert!(preview.contains("--sandbox workspace-write"));
        assert!(preview.contains("--worktree"));
        assert!(preview.contains("--profile fast"));
        assert!(preview.contains("--ephemeral"));
        let config_pretty = plan.config_overrides_pretty().unwrap();
        assert!(config_pretty.contains("model_reasoning_effort = \"high\""));
        assert!(config_pretty.contains("sandbox_workspace_write.network_access = true"));
        assert!(config_pretty.contains("mcp_oauth_callback_port = 8484"));
        assert!(config_pretty.contains("features.memories = true"));
        assert!(config_pretty.contains("future_option = true"));
    }

    // Reopen against the same SQLite DB and verify all 10 overrides persisted, then Reset All.
    let mut reopened =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    reopened.press_key("mod+,").unwrap();
    select_provider(&mut reopened, "role:button[Codex]", "id:prefs-codex-remove");
    reopened.wait_for("10 overrides").unwrap();
    reopened
        .press("id:prefs-codex-spawn-preview-toggle")
        .unwrap();
    reopened.wait_for("Resolved spawn invocation").unwrap();
    assert!(
        reopened
            .screen()
            .shows("Generated -c config.toml overrides")
    );

    reopened.press("id:prefs-codex-spawn-reset-all").unwrap();
    reopened.wait_for("All defaults").unwrap();
    assert!(
        !reopened
            .screen()
            .shows("Generated -c config.toml overrides")
    );

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let after_reset = db.load_preferences().unwrap().unwrap();
    assert!(after_reset.codex_spawn_settings.is_empty());
    let reset_plan = after_reset
        .build_codex_spawn_plan(&factory_core::CodexSpawnOptions::default())
        .unwrap();
    assert!(
        reset_plan
            .args
            .windows(2)
            .any(|w| w == ["-c", "check_for_update_on_startup=false"])
    );
}

#[test]
fn appearance_font_preferences_default_to_atkinson_and_persist_user_overrides() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");

    {
        let mut app = start_with_db(db_path.clone());
        app.press_key("mod+,").unwrap();
        app.wait_for("id:prefs-tab-appearance").unwrap();
        app.press("id:prefs-tab-appearance").unwrap();
        app.wait_for("UI font").unwrap();
        assert!(app.screen().shows("Code & monospace font"));
        assert!(
            app.screen().shows(
                "Default typeface for menus, labels, and app UI (Atkinson Hyperlegible Next)."
            )
        );
        assert!(app.screen().shows(
            "Monospace typeface for code, logs, commands, and keys (Atkinson Hyperlegible Mono)."
        ));

        // Verify native Font Panel picker triggers exist and are safe in headless tests.
        app.press("id:prefs-ui-font-choose").unwrap();
        app.press("id:prefs-mono-font-choose").unwrap();

        // Switch UI font to System and Code & monospace font to Custom("JetBrains Mono").
        app.press("id:prefs-ui-font-system").unwrap();
        app.press("id:prefs-mono-font-custom").unwrap();
        app.wait_for("id:prefs-mono-font-input").unwrap();
        app.type_text("id:prefs-mono-font-input", "JetBrains Mono")
            .unwrap();
        app.press("id:prefs-mono-font-apply").unwrap();

        app.press_window_close("preferences").unwrap();
    }

    // Verify persisted font overrides in SQLite.
    {
        let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
        let saved = db.load_preferences().unwrap().unwrap();
        assert_eq!(
            saved.appearance.ui_font,
            factory_core::FontPreference::System
        );
        assert_eq!(
            saved.appearance.mono_font,
            factory_core::FontPreference::Custom("JetBrains Mono".to_owned())
        );
    }

    // Reopen app and verify custom monospace font input stays populated, then switch back to Atkinson.
    let mut reopened = start_with_db(db_path.clone());
    reopened.press_key("mod+,").unwrap();
    reopened.wait_for("id:prefs-tab-appearance").unwrap();
    reopened.press("id:prefs-tab-appearance").unwrap();
    reopened.wait_for("id:prefs-mono-font-input").unwrap();
    reopened.press("id:prefs-ui-font-default").unwrap();
    reopened.press("id:prefs-mono-font-default").unwrap();

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let restored = db.load_preferences().unwrap().unwrap();
    assert_eq!(
        restored.appearance.ui_font,
        factory_core::FontPreference::Default
    );
    assert_eq!(
        restored.appearance.mono_font,
        factory_core::FontPreference::Default
    );
}

#[cfg(unix)]
#[test]
fn antigravity_cli_is_added_lists_models_and_usage_and_is_removed() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
echo '{"loggedIn":false,"authMethod":"none","configDirectory":"/tmp/.claude"}'
exit 1
"#,
    );
    write_fake_claude(
        &bin_dir,
        "agy",
        r#"#!/bin/sh
[ "$AGY_CLI_DISABLE_AUTO_UPDATE" = true ] || exit 9
case "$*" in
  models)
    printf 'gemini-4-neon-high\tGemini 4 Neon (High)\n'
    printf 'gemini-3.8-flash-low\tGemini 3.8 Flash (Low)\n'
    ;;
  *--print=/usage*)
    echo '{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[{"name":"Gemini Models","buckets":[{"window":"5h","remaining_fraction":0.62,"reset_time":"2099-11-24T17:15:19Z"}]}]}}}'
    ;;
esac
"#,
    );

    let mut app = start_with_db_and_claude_path(db_path, bin_dir.as_os_str().to_os_string());
    app.press_key("mod+,").unwrap();
    app.wait_for("No Antigravity account configured").unwrap();

    app.press("role:button[Add Account]").unwrap();
    app.wait_for("Add Antigravity CLI account...").unwrap();
    app.press("role:button[Add Antigravity CLI account...]")
        .unwrap();
    app.wait_for("Add Antigravity CLI account").unwrap();
    app.press("id:prefs-new-antigravity-submit").unwrap();

    // The probe proves the login; the details refresh reads the quota.
    app.wait_for("62% remaining").unwrap();
    assert!(app.screen().shows("5-hour (Gemini Models)"));
    assert!(
        !app.screen()
            .matches("id:prefs-antigravity-sign-in")
            .unwrap()
    );

    app.press("id:prefs-cli-model-change").unwrap();
    app.wait_for("Available from Antigravity CLI (2 models)")
        .unwrap();
    assert!(app.screen().shows("Gemini 3.8 Flash (Low)"));
    app.press("id:prefs-antigravity-family-flash").unwrap();
    app.wait_for("Showing 1 of 2 models from Antigravity CLI (Flash hidden)")
        .unwrap();

    app.press("id:prefs-antigravity-remove").unwrap();
    app.wait_for("id:prefs-antigravity-confirm-remove").unwrap();
    app.press("id:prefs-antigravity-confirm-remove").unwrap();
    app.wait_for("No Antigravity account configured").unwrap();
}

#[test]
fn project_mcp_servers_need_approval_and_users_can_add_their_own() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let repo = temp.path().join("mcp-service");
    std::fs::create_dir_all(repo.join(".agents")).unwrap();
    std::fs::write(
        repo.join(".agents/mcp_config.json"),
        r#"{"mcpServers":{"playwright":{"command":"npx","args":["-y","@playwright/mcp"]}}}"#,
    )
    .unwrap();

    let mut app = start_with_db(db_path.clone());
    app.press("role:button[Register Project]").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "mcp-service").unwrap();
    app.type_text("text:Repository path", &repo.display().to_string())
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'mcp-service'").unwrap();

    app.press_key("mod+shift+,").unwrap();
    app.wait_for("mcp-service · Issues & intake").unwrap();
    app.press("id:ps-tab-mcp-servers").unwrap();
    app.wait_for("Project servers").unwrap();
    assert!(app.screen().shows(".agents/mcp_config.json"));
    assert!(app.screen().shows("npx -y @playwright/mcp"));
    assert!(app.screen().shows(
        "These servers run commands from the repository. Review them, then approve to let agents use them."
    ));

    app.press("id:ps-mcp-approve").unwrap();
    app.wait_for("Approved").unwrap();
    assert!(app.screen().matches("id:ps-mcp-revoke").unwrap());

    app.press("id:mcp-add-server").unwrap();
    app.wait_for("id:mcp-form-name").unwrap();
    app.type_text("id:mcp-form-name", "docs").unwrap();
    app.type_text("id:mcp-form-target", "https://docs.example/mcp")
        .unwrap();
    app.type_text("id:mcp-form-pairs", "Authorization: Bearer ${DOCS_TOKEN}")
        .unwrap();
    app.press("id:mcp-form-save").unwrap();
    app.wait_for("Added MCP server 'docs'.").unwrap();
    assert!(app.screen().shows("https://docs.example/mcp"));
    assert!(app.screen().shows("Needs DOCS_TOKEN"));

    // Approval and the user's server are kept in bruk.db.
    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let project = db.list_projects().unwrap().into_iter().next().unwrap();
    let settings = db.load_project_settings(project.id).unwrap().unwrap();
    assert_eq!(settings.mcp_servers.len(), 1);
    let config = factory_core::mcp::load_project_mcp(&repo);
    assert_eq!(
        settings.approved_mcp_fingerprint.as_deref(),
        config.config().map(|c| c.fingerprint.as_str())
    );
}

#[test]
fn secrets_preferences_and_project_environment_flows() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("bruk.db");
    let repo = temp.path().join("secret-demo");
    std::fs::create_dir_all(&repo).unwrap();

    let mut app = start_with_db(db_path.clone());

    // Register a project first.
    app.press("role:button[Register Project]").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "secret-demo").unwrap();
    app.type_text("text:Repository path", &repo.display().to_string())
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'secret-demo'").unwrap();

    // Open Preferences -> Secrets tab and add an App Env secret.
    app.press_key("mod+,").unwrap();
    app.wait_for("id:prefs-tab-secrets").unwrap();
    app.press("id:prefs-tab-secrets").unwrap();
    app.wait_for("id:prefs-secrets-store-status").unwrap();
    assert!(app.screen().shows("Keychain available"));

    app.press("id:prefs-secrets-add-app-btn").unwrap();
    app.wait_for("id:prefs-secrets-app-name-input").unwrap();
    app.type_text("id:prefs-secrets-app-name-input", "SHARED_API_TOKEN")
        .unwrap();
    app.type_text(
        "id:prefs-secrets-app-value-input",
        "sk_live_app_secret_99887766",
    )
    .unwrap();
    app.press("id:prefs-secrets-app-save").unwrap();
    app.wait_for("Saved app secret 'SHARED_API_TOKEN' in Keychain.")
        .unwrap();
    assert!(app.screen().shows("SHARED_API_TOKEN"));
    // Plaintext secret value must NEVER be shown on screen.
    assert!(!app.screen().shows("sk_live_app_secret_99887766"));

    // Open Project Settings -> Environment via the Project menu shortcut (⇧⌘,).
    app.press_window_close("preferences").unwrap();
    app.press_key("mod+shift+,").unwrap();
    app.wait_for("secret-demo · Issues & intake").unwrap();
    app.press("id:ps-tab-environment").unwrap();
    app.wait_for("Inherited from App Secrets").unwrap();
    assert!(app.screen().shows("SHARED_API_TOKEN"));

    // Exclude the inherited App secret for this project, then add a project secret.
    app.press("id:ps-inherited-exclude").unwrap();
    app.wait_for("Excluded in this project").unwrap();

    app.press("id:ps-env-add-btn").unwrap();
    app.wait_for("id:ps-env-key-input").unwrap();
    app.type_text("id:ps-env-key-input", "PROJ_SECRET_KEY")
        .unwrap();
    app.type_text("id:ps-env-value-input", "proj_super_secret_55443322")
        .unwrap();
    app.press("id:ps-env-secret-checkbox").unwrap();
    app.press("id:ps-env-save").unwrap();
    app.wait_for("Saved secret 'PROJ_SECRET_KEY' in Keychain.")
        .unwrap();
    assert!(app.screen().shows("PROJ_SECRET_KEY"));
    assert!(!app.screen().shows("proj_super_secret_55443322"));

    // Verify SQLite stores metadata only and zero plaintext secret bytes!
    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let records = db.list_secrets().unwrap();
    assert_eq!(records.len(), 2);
    let raw_db_bytes = std::fs::read(&db_path).unwrap();
    let raw_db_text = String::from_utf8_lossy(&raw_db_bytes);
    assert!(!raw_db_text.contains("sk_live_app_secret_99887766"));
    assert!(!raw_db_text.contains("proj_super_secret_55443322"));
}
