//! Headless Native Shell UI tests for first-launch home setup, git clone/init, and chat flows.

mod common;

use tempfile::TempDir;

#[allow(unused_imports)]
use common::*;

#[test]
fn first_launch_sets_up_custom_bruk_folder_imports_legacy_data_and_remembers_it() {
    let temp = TempDir::new().unwrap();
    let environment = isolated_home_environment(temp.path());
    let legacy_db = environment.legacy_database_paths[0].clone();
    {
        let db = factory_core::ProjectDatabase::open(&legacy_db).unwrap();
        db.register_project("legacy-api", "/workspace/legacy-api", "main", "")
            .unwrap();
    }
    let custom_root = temp.path().join("Data").join("Bruk");

    {
        let mut app = start_with_home_environment(environment.clone());
        app.wait_for("Welcome to Bruk").unwrap();
        assert!(
            app.screen()
                .shows("Import existing projects and preferences")
        );
        assert!(app.screen().shows("Create Bruk Folder"));

        app.clear("id:home-setup-root").unwrap();
        app.type_text("id:home-setup-root", &custom_root.display().to_string())
            .unwrap();
        app.press("id:home-setup-continue").unwrap();
        app.wait_for(&format!(
            "Bruk folder ready at {} · imported projects and preferences from {}",
            factory_core::display_with_tilde(&custom_root),
            factory_core::display_with_tilde(&legacy_db),
        ))
        .unwrap();
        app.wait_for("legacy-api").unwrap();
        assert!(!app.screen().shows("Welcome to Bruk"));
    }

    assert!(custom_root.join("bruk.db").is_file());
    assert!(custom_root.join("projects").is_dir());
    assert!(custom_root.join("runs").is_dir());
    assert_eq!(
        std::fs::read_to_string(&environment.locator_path)
            .unwrap()
            .trim(),
        custom_root.display().to_string()
    );
    assert!(!environment.default_root.exists());
    assert!(legacy_db.is_file(), "legacy database must be left in place");

    // Relaunching opens the remembered folder directly.
    let mut reopened = start_with_home_environment(environment);
    reopened.wait_for("legacy-api").unwrap();
    assert!(!reopened.screen().shows("Welcome to Bruk"));
}

#[test]
fn first_launch_default_folder_without_legacy_data_skips_import() {
    let temp = TempDir::new().unwrap();
    let environment = isolated_home_environment(temp.path());

    let mut app = start_with_home_environment(environment.clone());
    app.wait_for("Welcome to Bruk").unwrap();
    assert!(
        !app.screen()
            .shows("Import existing projects and preferences")
    );
    app.press("id:home-setup-continue").unwrap();
    app.wait_for("Dashboard").unwrap();

    assert!(environment.default_root.join("bruk.db").is_file());
    assert!(
        !environment.locator_path.exists(),
        "the default folder needs no locator file"
    );
}

#[test]
fn clones_repository_into_bruk_folder_registers_it_and_keeps_clone_on_remove() {
    let temp = TempDir::new().unwrap();
    let home_root = temp.path().join("Bruk");
    let db_path = home_root.join("bruk.db");
    let origin = init_origin_repo(&temp.path().join("origin").join("billing-api"));

    let clone = home_root.join("projects").join("billing-api");
    let origin_str = origin.display().to_string();

    let mut app = start_with_db(db_path.clone());
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    assert!(
        !app.screen()
            .enabled("role:button[Clone Repository]")
            .unwrap()
    );

    // The project name follows the repository name and previews the destination.
    app.type_text("id:register-clone-source", &origin_str)
        .unwrap();
    app.wait_for(&format!(
        "Clones into {}",
        factory_core::display_with_tilde(&clone)
    ))
    .unwrap();
    app.press("role:button[Clone Repository]").unwrap();
    app.wait_for("Registered project 'billing-api'").unwrap();
    assert!(app.screen().shows("In Bruk folder"));
    assert!(clone.join("README.md").is_file());

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let projects = db.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].root_path, clone);
    assert_eq!(projects[0].default_branch, "trunk");

    // Cloning into an occupied folder is refused with guidance.
    app.type_text("id:register-clone-source", &origin_str)
        .unwrap();
    app.press("role:button[Clone Repository]").unwrap();
    app.wait_for(&format!(
        "'{}' already exists. Link it as an existing clone or choose another project name.",
        clone.display()
    ))
    .unwrap();

    // Removing the project leaves the clone on disk.
    app.press("role:button[Remove]").unwrap();
    app.wait_for(&format!(
        "Removed project 'billing-api'. Its clone is still at {}.",
        factory_core::display_with_tilde(&clone)
    ))
    .unwrap();
    assert!(clone.join("README.md").is_file());
}

#[test]
fn default_projects_dir_follows_bruk_folder_and_redirects_clones_when_changed() {
    let temp = TempDir::new().unwrap();
    let home_root = temp.path().join("Bruk");
    let db_path = home_root.join("bruk.db");
    let origin = init_origin_repo(&temp.path().join("origin").join("billing-api"));
    let origin_str = origin.display().to_string();
    let custom_dir = temp.path().join("src");
    std::fs::create_dir_all(&custom_dir).unwrap();
    let saved_projects_dir = || {
        factory_core::ProjectDatabase::open(&db_path)
            .unwrap()
            .load_preferences()
            .unwrap()
            .map(|prefs| prefs.general.default_projects_dir)
    };

    let mut app = start_with_db(db_path.clone());

    // By default, clones land in the Bruk folder's projects/.
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.wait_for(&format!(
        "Clones into {}/",
        factory_core::display_with_tilde(&home_root.join("projects"))
    ))
    .unwrap();

    // Point the default projects directory somewhere else in Preferences › General.
    app.press_key("mod+,").unwrap();
    app.press("id:prefs-tab-general").unwrap();
    app.wait_for("Default projects directory").unwrap();
    app.clear("id:prefs-default-projects-dir").unwrap();
    app.type_text(
        "id:prefs-default-projects-dir",
        &custom_dir.display().to_string(),
    )
    .unwrap();
    app.press("role:button[Apply]").unwrap();
    assert_eq!(
        saved_projects_dir().as_deref(),
        Some(custom_dir.display().to_string().as_str())
    );
    app.press_window_close("preferences").unwrap();

    // Clones now go to the custom directory.
    app.type_text("id:register-clone-source", &origin_str)
        .unwrap();
    let custom_clone = custom_dir.join("billing-api");
    app.wait_for(&format!(
        "Clones into {}",
        factory_core::display_with_tilde(&custom_clone)
    ))
    .unwrap();
    app.press("role:button[Clone Repository]").unwrap();
    app.wait_for("Registered project 'billing-api'").unwrap();
    assert!(custom_clone.join("README.md").is_file());
    assert!(!home_root.join("projects").join("billing-api").exists());

    // Clearing the field goes back to following the Bruk folder.
    app.press_key("mod+,").unwrap();
    app.press("id:prefs-tab-general").unwrap();
    app.wait_for("Default projects directory").unwrap();
    app.clear("id:prefs-default-projects-dir").unwrap();
    app.press("role:button[Apply]").unwrap();
    assert_eq!(saved_projects_dir().as_deref(), Some(""));
    app.press_window_close("preferences").unwrap();
    app.wait_for(&format!(
        "Clones into {}/",
        factory_core::display_with_tilde(&home_root.join("projects"))
    ))
    .unwrap();
}

#[test]
fn creates_new_local_repository_in_bruk_folder_and_custom_location() {
    use std::process::{Command, Stdio};

    let temp = TempDir::new().unwrap();
    let home_root = temp.path().join("Bruk");
    let db_path = home_root.join("bruk.db");
    let custom_dir = temp.path().join("my-repos");
    std::fs::create_dir_all(&custom_dir).unwrap();

    let default_repo = home_root.join("projects").join("local-tool");
    let custom_repo = custom_dir.join("scratch-pad");

    let mut app = start_with_db(db_path.clone());
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-create").unwrap();
    app.wait_for(&format!(
        "Creates in {}/",
        factory_core::display_with_tilde(&home_root.join("projects"))
    ))
    .unwrap();
    assert!(!app.screen().enabled("id:register-create-submit").unwrap());

    // Create a local repository in the default Bruk projects folder on a custom default branch.
    app.type_text("id:register-create-name", "local-tool")
        .unwrap();
    app.wait_for(&format!(
        "Creates {}",
        factory_core::display_with_tilde(&default_repo)
    ))
    .unwrap();
    app.clear("id:register-create-branch").unwrap();
    app.type_text("id:register-create-branch", "trunk").unwrap();
    app.press("id:register-create-submit").unwrap();
    app.wait_for("Registered project 'local-tool'").unwrap();
    assert!(app.screen().shows("In Bruk folder"));
    assert!(default_repo.join(".git").is_dir());
    assert_eq!(
        std::fs::read_to_string(default_repo.join("README.md")).unwrap(),
        "# local-tool\n"
    );
    assert!(
        Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(&default_repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );

    // Creating a second repository with the same name in the same location is refused.
    app.type_text("id:register-create-name", "local-tool")
        .unwrap();
    app.press("id:register-create-submit").unwrap();
    app.wait_for(&format!(
        "A project is already registered at '{}'.",
        default_repo.display()
    ))
    .unwrap();

    // Create another local repository in a custom folder without a starter README.
    app.clear("id:register-create-name").unwrap();
    app.type_text("id:register-create-name", "scratch-pad")
        .unwrap();
    app.type_text(
        "id:register-create-location",
        &custom_dir.display().to_string(),
    )
    .unwrap();
    app.wait_for(&format!(
        "Creates {}",
        factory_core::display_with_tilde(&custom_repo)
    ))
    .unwrap();
    app.press("id:register-create-readme").unwrap();
    app.press("id:register-create-submit").unwrap();
    app.wait_for("Registered project 'scratch-pad'").unwrap();
    assert!(custom_repo.join(".git").is_dir());
    assert!(!custom_repo.join("README.md").exists());
    assert!(
        Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(&custom_repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let projects = db.list_projects().unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].name, "local-tool");
    assert_eq!(projects[0].root_path, default_repo);
    assert_eq!(projects[0].default_branch, "trunk");
    assert_eq!(projects[1].name, "scratch-pad");
    assert_eq!(projects[1].root_path, custom_repo);
    assert_eq!(projects[1].default_branch, "main");
}

#[cfg(unix)]
#[test]
fn chat_sessions_stream_replies_resume_the_agent_session_and_persist() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let repo_dir = temp.path().join("chat-repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let log = temp.path().join("claude-calls.log");

    let context_log = temp.path().join("claude-context.log");

    // Chat turns are the only calls with `--include-partial-messages`, and context readings
    // the only ones with `--fork-session`; anything else is the app's own account and
    // details probing.
    write_fake_claude(
        &bin_dir,
        "claude",
        &format!(
            r#"#!/bin/sh
case " $* " in
  *" --fork-session "*)
    echo "CONTEXT: $* STDIN: $(cat)" >> '{context_log}'
    echo '{{"type":"control_response","response":{{"subtype":"success","request_id":"bruk_context_usage","response":{{"categories":[{{"id":"system_prompt","name":"System prompt","tokens":3000,"kind":"used"}},{{"id":"messages","name":"Messages","tokens":1234,"kind":"used"}},{{"id":"autocompact_buffer","name":"Autocompact buffer","tokens":33000,"kind":"buffer"}},{{"id":"free_space","name":"Free space","tokens":162766,"kind":"free"}}],"totalTokens":4234,"maxTokens":200000,"model":"claude-test","memoryFiles":[{{"path":"/repo/CLAUDE.md","tokens":120}}],"autoCompactThreshold":167000,"isAutoCompactEnabled":true}}}}}}'
    ;;
  *" --include-partial-messages "*)
    prompt=$(cat)
    echo "ARGS: $*" >> '{log}'
    echo "PROMPT: $prompt" >> '{log}'
    echo '{{"type":"system","subtype":"init","session_id":"sess-42"}}'
    case "$prompt" in
      *"Show me the tests"*)
        # A later turn's requests report how full the window is while the reply runs.
        echo '{{"type":"stream_event","event":{{"type":"message_start","message":{{"usage":{{"input_tokens":12,"cache_read_input_tokens":6488}}}}}}}}'
        sleep 3
        ;;
    esac
    echo '{{"type":"assistant","message":{{"content":[{{"type":"thinking","thinking":"Auth lives in src/auth.rs, check it first."}}]}}}}'
    echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"toolu_1","name":"Read","input":{{"file_path":"src/auth.rs"}}}}]}}}}'
    echo '{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"toolu_1","content":"fn login()\\n    check_password()"}}]}}}}'
    echo "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"Reply to: $prompt\"}}]}}}}"
    echo '{{"type":"result","subtype":"success","is_error":false,"result":"done","total_cost_usd":0.02,"usage":{{"input_tokens":1200,"output_tokens":34}},"model_usage_since_last_result":{{"claude-test":{{"inputTokens":1200,"outputTokens":34,"costUSD":0.02}}}}}}'
    ;;
  *)
    echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"chat@example.com"}}'
    ;;
esac
"#,
            log = log.display(),
            context_log = context_log.display()
        ),
    );

    {
        let mut app =
            start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
        app.press("id:dashboard-empty-register-project").unwrap();
        app.wait_for("Add a project").unwrap();
        app.press("id:register-mode-link").unwrap();
        app.type_text("text:Project name", "chat-repo").unwrap();
        app.type_text("text:Repository path", &repo_dir.display().to_string())
            .unwrap();
        app.press("role:button[Link Project]").unwrap();
        app.wait_for("Registered project 'chat-repo'").unwrap();

        // Chats sit beside the project's workflows with a fresh composer.
        app.press("id:sidebar-chats-1").unwrap();
        app.wait_for("Chat with an agent in chat-repo").unwrap();
        assert!(app.screen().shows("chat-repo · Chats"));
        assert!(
            app.screen()
                .shows("No chats yet. Ask an agent about this project, or have it make a change.")
        );
        assert!(!app.screen().enabled("id:chat-send-btn").unwrap());

        // The first message starts the chat, names it, and streams the reply with its tool calls.
        app.type_text("id:chat-composer-input", "Where is auth handled")
            .unwrap();
        app.press("id:chat-send-btn").unwrap();
        app.wait_for("Reply to: Where is auth handled").unwrap();
        assert!(app.screen().shows("src/auth.rs"));
        // Tool calls and thinking start folded; opening one shows all it took and gave back.
        assert!(app.screen().shows("Thinking"));
        assert!(!app.screen().shows("fn login()"));
        assert!(!app.screen().matches("id:chat-tool-2-details").unwrap());
        app.press("id:chat-tool-2").unwrap();
        app.wait_for("id:chat-tool-2-details").unwrap();
        assert!(app.screen().shows("file_path: src/auth.rs"));
        assert!(app.screen().shows("fn login()\n    check_password()"));
        app.press("id:chat-tool-2").unwrap();
        assert!(!app.screen().matches("id:chat-tool-2-details").unwrap());
        app.press("id:chat-reasoning-1").unwrap();
        app.wait_for("Auth lives in src/auth.rs, check it first.")
            .unwrap();
        app.wait_for("Claude · 1 message · 1K tokens · $0.02")
            .unwrap();
        assert!(
            app.screen()
                .matches("role:button[Where is auth handled]")
                .unwrap()
        );
        app.wait_for("id:chat-send-btn").unwrap();
        assert!(
            app.screen().shows(
                "Return to send · Shift+Return for a new line · Drop or paste files to attach · Works in the project checkout"
            )
        );

        // After the reply Bruk measures the context window; the panel breaks it down.
        app.wait_for("4.2K / 200K").unwrap();
        app.press("id:chat-context-btn").unwrap();
        app.wait_for("4.2K of 200K tokens").unwrap();
        assert!(app.screen().matches("id:chat-context-table").unwrap());
        assert!(
            app.screen()
                .matches("id:chat-context-table-row-messages")
                .unwrap()
        );
        for shown in [
            "Context window",
            "Portion",
            "Tokens",
            "Share",
            "2% full · claude-test",
            "System prompt",
            "Messages",
            "Autocompact buffer",
            "Free space",
            "Memory files · 1",
            "/repo/CLAUDE.md",
            // What the chat used, by kind.
            "Tokens used in this chat",
            "Input",
            "Output",
            "Cache reads",
            "Cache writes",
            "1,200",
            "1,234",
        ] {
            assert!(app.screen().shows(shown), "missing {shown}");
        }
        assert!(app.screen().matches("id:chat-tokens-table").unwrap());
        assert!(!app.screen().shows("Live"));

        // Return sends the next message, which resumes the same agent session.
        app.type_text("id:chat-composer-input", "Show me the tests")
            .unwrap();
        app.submit("id:chat-composer-input").unwrap();
        // The window's count and the chat's tokens follow the running reply, then settle
        // after it.
        app.wait_for("6.5K / 200K").unwrap();
        app.wait_for("7,734").unwrap();
        assert!(app.screen().matches("id:chat-tokens-live").unwrap());
        assert!(app.screen().shows("6,488"));
        app.wait_for("Reply to: Show me the tests").unwrap();
        app.wait_for("4.2K / 200K").unwrap();
        app.wait_for("2,468").unwrap();
        app.press("id:chat-context-close").unwrap();
        assert!(!app.screen().shows("4.2K of 200K tokens"));
        app.wait_for("Claude · 2 messages · 2K tokens · $0.04")
            .unwrap();
        assert!(app.screen().shows("Reply to: Where is auth handled"));

        // The Dashboard counts what the chat used.
        app.press("id:sidebar-dashboard-btn").unwrap();
        app.wait_for("Highest token runs & chats").unwrap();
        app.wait_for("chat-repo · Chat · Claude").unwrap();
        assert!(app.screen().shows("$0.04 · 2 replies"));
        app.press("id:sidebar-chats-1").unwrap();
        app.wait_for("Reply to: Show me the tests").unwrap();

        // A new chat leaves the first one in the list.
        app.press("id:chat-new-btn").unwrap();
        app.wait_for("Chat with an agent in chat-repo").unwrap();
        assert!(
            app.screen()
                .matches("role:button[Where is auth handled]")
                .unwrap()
        );
    }

    let calls = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    assert_eq!(lines.len(), 4, "{calls}");
    assert!(!lines[0].contains("--resume"), "{}", lines[0]);
    assert!(
        lines[0].contains("--setting-sources project"),
        "{}",
        lines[0]
    );
    assert_eq!(lines[1], "PROMPT: Where is auth handled");
    assert!(lines[2].ends_with("--resume sess-42"), "{}", lines[2]);
    assert_eq!(lines[3], "PROMPT: Show me the tests");

    // Each reply is followed by one context reading of an unsaved fork of the session.
    let context_calls = std::fs::read_to_string(&context_log).unwrap();
    let context_lines: Vec<&str> = context_calls.lines().collect();
    assert_eq!(context_lines.len(), 2, "{context_calls}");
    for line in &context_lines {
        assert!(
            line.contains("--resume sess-42 --fork-session --no-session-persistence"),
            "{line}"
        );
        assert!(line.contains(r#""subtype":"get_context_usage""#), "{line}");
    }

    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let chats = db.list_chat_sessions().unwrap();
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].title, "Where is auth handled");
    assert_eq!(chats[0].provider_session_id.as_deref(), Some("sess-42"));
    assert_eq!(chats[0].status, factory_core::ChatSessionStatus::Idle);
    assert_eq!(
        chats[0]
            .context
            .as_ref()
            .map(|c| (c.used_tokens, c.window_tokens)),
        Some((4_234, 200_000))
    );
    let usage = db.list_chat_usage().unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(
        (usage[0].tokens, usage[0].cost_micros, usage[0].turns),
        (2_468, 40_000, 2)
    );
    drop(db);

    // Chats survive a restart and can be deleted.
    let mut reopened = start_with_db_and_claude_path(db_path.clone(), bin_dir.into_os_string());
    reopened.press("id:sidebar-chats-1").unwrap();
    reopened
        .wait_for("role:button[Where is auth handled]")
        .unwrap();
    reopened
        .press("role:button[Where is auth handled]")
        .unwrap();
    reopened.wait_for("Reply to: Show me the tests").unwrap();
    reopened.press("id:chat-delete-btn").unwrap();
    reopened.press("id:chat-delete-confirm").unwrap();
    reopened
        .wait_for("Deleted chat 'Where is auth handled'")
        .unwrap();
    assert!(reopened.screen().shows("Chat with an agent in chat-repo"));
    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    assert!(db.list_chat_sessions().unwrap().is_empty());

    // What a deleted chat used still counts.
    reopened.press("id:sidebar-dashboard-btn").unwrap();
    reopened.wait_for("$0.04 · 2 replies · deleted").unwrap();
    assert!(db.list_chat_usage().unwrap()[0].deleted);
}

#[cfg(unix)]
#[test]
fn chat_attachments_drop_paste_and_send_images_to_the_agent() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let repo_dir = temp.path().join("attach-repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let log = temp.path().join("claude-calls.log");
    write_fake_claude(
        &bin_dir,
        "claude",
        &format!(
            r#"#!/bin/sh
case " $* " in
  *" --fork-session "*) cat > /dev/null ;;
  *" --include-partial-messages "*)
    echo "ARGS: $*" >> '{log}'
    printf 'STDIN: %s\n' "$(cat)" >> '{log}'
    echo '{{"type":"system","subtype":"init","session_id":"sess-img"}}'
    echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"I see a red square."}}]}}}}'
    echo '{{"type":"result","subtype":"success","is_error":false,"result":"done"}}'
    ;;
  *) echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"chat@example.com"}}' ;;
esac
"#,
            log = log.display()
        ),
    );
    let drops = temp.path().join("drops");
    std::fs::create_dir_all(&drops).unwrap();
    let screenshot = drops.join("red square.png");
    std::fs::write(&screenshot, [0x89, b'P', b'N', b'G']).unwrap();
    let notes = drops.join("notes.txt");
    std::fs::write(&notes, "hello").unwrap();

    let mut app = start_with_db_and_claude_path(db_path.clone(), bin_dir.into_os_string());
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "attach-repo").unwrap();
    app.type_text("text:Repository path", &repo_dir.display().to_string())
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'attach-repo'").unwrap();
    app.press("id:sidebar-chats-1").unwrap();
    app.wait_for("Chat with an agent in attach-repo").unwrap();

    // Files dropped on the chat wait in the composer; Send works with no text.
    app.drop_files(
        "id:chat-drop-zone",
        vec![
            "file://".to_owned() + &screenshot.display().to_string().replace(' ', "%20"),
            format!("file://{}", notes.display()),
            format!("file://{}", drops.display()),
        ],
    )
    .unwrap();
    app.wait_for("id:chat-pending-attachments").unwrap();
    assert_eq!(app.screen().count("id:chat-pending-attachment").unwrap(), 2);
    assert!(app.screen().shows("notes.txt"));
    assert!(app.screen().shows("Text · 5 B"));
    assert!(app.screen().shows("Couldn't attach: drops is a folder."));
    assert!(app.screen().enabled("id:chat-send-btn").unwrap());

    // A pasted screenshot attaches too; a file can be taken off again.
    let mut paste = nsh_core::PasteEvent::new("");
    paste
        .files
        .push(nsh_core::PastedFile::image(vec![0x89, b'P', b'N', b'G']));
    app.paste("id:chat-composer-input", &paste).unwrap();
    app.wait_until(
        "the pasted image",
        std::time::Duration::from_secs(5),
        |screen| screen.count("id:chat-pending-attachment").unwrap() == 3,
    )
    .unwrap();
    app.press("role:button[Remove notes.txt]").unwrap();
    assert_eq!(app.screen().count("id:chat-pending-attachment").unwrap(), 2);

    app.type_text("id:chat-composer-input", "What is in these?")
        .unwrap();
    app.press("id:chat-send-btn").unwrap();
    app.wait_for("I see a red square.").unwrap();
    assert!(!app.screen().matches("id:chat-pending-attachments").unwrap());
    assert_eq!(app.screen().count("id:chat-sent-attachment").unwrap(), 2);

    let calls = std::fs::read_to_string(&log).unwrap();
    let mut lines = calls.lines();
    let args = lines.next().unwrap();
    assert!(args.contains("--input-format stream-json"), "{args}");
    assert!(args.contains("--add-dir="), "{args}");
    let message: serde_json::Value =
        serde_json::from_str(lines.next().unwrap().strip_prefix("STDIN: ").unwrap()).unwrap();
    let content = message["message"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 3, "the text, then both images: {message}");
    let text = content[0]["text"].as_str().unwrap();
    assert!(text.starts_with("What is in these?"), "{text}");
    assert!(text.contains("red square.png"), "{text}");
    assert!(!text.contains("notes.txt"), "{text}");

    // The transcript keeps the chat's own copies, which outlive the originals.
    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let chats = db.list_chat_sessions().unwrap();
    let Some(factory_core::ChatEntry::User { attachments, .. }) = chats[0].entries.first() else {
        panic!("no user message: {:?}", chats[0].entries);
    };
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0].name, "red square.png");
    assert_eq!(attachments[1].name, "image.png");
    std::fs::remove_file(&screenshot).unwrap();
    assert!(attachments.iter().all(|a| a.path.is_file()));
    assert!(attachments[0].path.ends_with("attachments/red square.png"));
}

#[cfg(unix)]
#[test]
fn worktree_chats_work_on_their_own_branch_and_leave_the_checkout_alone() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let repo_dir = temp.path().join("wt-repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("README.md"), "demo\n").unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let log = temp.path().join("claude-calls.log");
    // The agent "edits" a file wherever it runs.
    write_fake_claude(
        &bin_dir,
        "claude",
        &format!(
            r#"#!/bin/sh
case " $* " in
  *" --include-partial-messages "*)
    cat > /dev/null
    echo "CWD: $(pwd -P)" >> '{log}'
    echo "edited" > agent-notes.txt
    echo '{{"type":"system","subtype":"init","session_id":"wt-sess"}}'
    echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"Wrote agent-notes.txt"}}]}}}}'
    echo '{{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{{"input_tokens":10,"output_tokens":2}}}}'
    ;;
  *" --fork-session "*)
    cat > /dev/null
    ;;
  *)
    echo '{{"loggedIn":true,"authMethod":"claude.ai","email":"chat@example.com"}}'
    ;;
esac
"#,
            log = log.display()
        ),
    );

    let mut app =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "wt-repo").unwrap();
    app.type_text("text:Repository path", &repo_dir.display().to_string())
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'wt-repo'").unwrap();

    app.press("id:sidebar-chats-1").unwrap();
    app.wait_for("Chat with an agent in wt-repo").unwrap();
    app.press("id:chat-new-workspace").unwrap();
    app.press("role:option[New worktree]").unwrap();
    app.wait_for("The agent gets a new branch from HEAD in a folder of its own, so your checkout stays as it is.")
        .unwrap();
    assert!(
        app.screen()
            .shows("Return to send · Shift+Return for a new line · Drop or paste files to attach · Works in a new worktree")
    );

    app.type_text("id:chat-composer-input", "Add release notes")
        .unwrap();
    app.press("id:chat-send-btn").unwrap();
    app.wait_for("Wrote agent-notes.txt").unwrap();
    app.wait_for(
        "Return to send · Shift+Return for a new line · Drop or paste files to attach · Works in worktree agent/chat-add-release-notes",
    )
        .unwrap();
    assert!(
        app.screen()
            .matches("role:button[Show worktree agent/chat-add-release-notes in Finder]")
            .unwrap()
    );

    // The edit landed in the chat's worktree, not in the checkout.
    let db = factory_core::ProjectDatabase::open(&db_path).unwrap();
    let chat = db.list_chat_sessions().unwrap().remove(0);
    drop(db);
    let worktree = chat.worktree.clone().unwrap();
    assert_eq!(worktree.branch, "agent/chat-add-release-notes");
    assert!(
        worktree
            .path
            .ends_with(format!("chats/{}/worktree", chat.id))
    );
    assert_eq!(
        std::fs::read_to_string(worktree.path.join("agent-notes.txt")).unwrap(),
        "edited\n"
    );
    assert!(!repo_dir.join("agent-notes.txt").exists());
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(
        calls.trim(),
        format!(
            "CWD: {}",
            std::fs::canonicalize(&worktree.path).unwrap().display()
        )
    );

    // Deleting the chat removes its worktree; the branch stays.
    app.press("id:chat-delete-btn").unwrap();
    app.wait_for("Delete the chat and its worktree? Uncommitted changes there are lost.")
        .unwrap();
    app.press("id:chat-delete-confirm").unwrap();
    app.wait_for("Deleted chat 'Add release notes' and its worktree. Branch agent/chat-add-release-notes is kept.")
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while worktree.path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!worktree.path.exists());
    let branches = std::process::Command::new("git")
        .args(["branch", "--list", "agent/chat-add-release-notes"])
        .current_dir(&repo_dir)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&branches.stdout).contains("agent/chat-add-release-notes"));
}

#[cfg(unix)]
#[test]
fn chat_changes_drawer_inspects_diffs_applies_commits_and_opens_pr() {
    let temp = TempDir::new().unwrap();
    let db_path = temp.path().join("factory.db");
    let bare_origin = temp.path().join("origin.git");
    std::fs::create_dir_all(&bare_origin).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&bare_origin)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );

    let repo_dir = temp.path().join("changes-repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("README.md"), "demo\n").unwrap();
    let origin_str = bare_origin.to_string_lossy().into_owned();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
        vec!["config", "commit.gpgsign", "false"],
        vec!["config", "tag.gpgsign", "false"],
        vec!["add", "."],
        vec!["commit", "-m", "initial"],
        vec!["remote", "add", "origin", &origin_str],
        vec!["push", "-u", "origin", "main"],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    write_fake_claude(
        &bin_dir,
        "claude",
        r#"#!/bin/sh
case " $* " in
  *" --include-partial-messages "*)
    prompt="$(cat)"
    case "$prompt" in
      *"create a git commit"*)
        git add -A
        git commit -m "Add hello module" > /dev/null
        echo '{"type":"system","subtype":"init","session_id":"wt-diff-sess"}'
        echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Committed the changes as Add hello module."}]}}'
        echo '{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{"input_tokens":8,"output_tokens":3}}'
        ;;
      *)
        printf 'demo\nupdated by agent\n' > README.md
        printf 'pub fn hello() -> &str { "hi" }\n' > hello.rs
        echo '{"type":"system","subtype":"init","session_id":"wt-diff-sess"}'
        echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Updated README.md and created hello.rs"}]}}'
        echo '{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{"input_tokens":12,"output_tokens":4}}'
        ;;
    esac
    ;;
  *" --fork-session "*)
    cat > /dev/null
    ;;
  *)
    echo '{"loggedIn":true,"authMethod":"claude.ai","email":"chat@example.com"}'
    ;;
esac
"#,
    );
    let gh_log = temp.path().join("gh-calls.log");
    write_fake_claude(
        &bin_dir,
        "gh",
        &format!(
            "#!/bin/sh\necho \"$*\" > '{}'\necho 'https://github.com/acme/changes-repo/pull/77'\n",
            gh_log.display()
        ),
    );

    let mut app =
        start_with_db_and_claude_path(db_path.clone(), bin_dir.as_os_str().to_os_string());
    app.press("id:dashboard-empty-register-project").unwrap();
    app.wait_for("Add a project").unwrap();
    app.press("id:register-mode-link").unwrap();
    app.type_text("text:Project name", "changes-repo").unwrap();
    app.type_text("text:Repository path", &repo_dir.display().to_string())
        .unwrap();
    app.press("role:button[Link Project]").unwrap();
    app.wait_for("Registered project 'changes-repo'").unwrap();

    // Start a worktree chat and let the agent edit README.md + create hello.rs.
    app.press("id:sidebar-chats-1").unwrap();
    app.wait_for("Chat with an agent in changes-repo").unwrap();
    app.press("id:chat-new-workspace").unwrap();
    app.press("role:option[New worktree]").unwrap();
    app.type_text("id:chat-composer-input", "Add hello module")
        .unwrap();
    app.press("id:chat-send-btn").unwrap();
    app.wait_for("Updated README.md and created hello.rs")
        .unwrap();

    // Header Changes button updates automatically with file count & diff stats.
    app.wait_for("2 files").unwrap();
    app.press("id:chat-changes-btn").unwrap();
    app.wait_for("id:chat-changes-panel").unwrap();
    app.wait_for("Uncommitted changes · 2 files").unwrap();
    assert!(app.screen().shows("README.md"));
    assert!(app.screen().shows("hello.rs"));
    assert!(app.screen().shows("New"));
    assert!(app.screen().shows("+updated by agent"));

    // Collapse all and Expand all toggle every file card in the diff view.
    assert!(app.screen().shows("Collapse all"));
    app.press("id:chat-diff-toggle-all").unwrap();
    app.wait_for("Expand all").unwrap();
    assert!(!app.screen().shows("+updated by agent"));
    app.press("id:chat-diff-toggle-all").unwrap();
    app.wait_for("Collapse all").unwrap();
    assert!(app.screen().shows("+updated by agent"));

    // Apply uncommitted worktree changes to the main project checkout.
    app.press("id:chat-changes-apply-btn").unwrap();
    app.wait_for("Applied 2 files to the project checkout.")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(repo_dir.join("README.md")).unwrap(),
        "demo\nupdated by agent\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo_dir.join("hello.rs")).unwrap(),
        "pub fn hello() -> &str { \"hi\" }\n"
    );

    // Pressing Commit sends a message to the agent to create the commit, and the drawer refreshes.
    app.press("id:chat-changes-commit-btn").unwrap();
    app.wait_for("Committed the changes as Add hello module.")
        .unwrap();
    app.wait_for("1 commit ahead of main").unwrap();
    assert!(app.screen().shows("Branch diff · 2 files"));

    // Push branch and open a Pull Request via the inline PR form.
    app.press("id:chat-changes-pr-toggle").unwrap();
    app.wait_for("id:chat-pr-form").unwrap();
    app.press("id:chat-pr-submit").unwrap();
    app.wait_for("id:chat-pr-link-card").unwrap();
    assert!(
        app.screen()
            .shows("https://github.com/acme/changes-repo/pull/77")
    );
    let gh_args = std::fs::read_to_string(&gh_log).unwrap();
    assert!(gh_args.contains("pr create"));
    assert!(gh_args.contains("Add hello module"));
}
