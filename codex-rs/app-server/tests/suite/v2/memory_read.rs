//! Exercises version selection through app-server and the model request boundary.

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ThreadMemoryReadResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn memory_read_version_selects_only_its_own_context() -> Result<()> {
    for (version, has_v1_workspace, expected, excluded, use_memories) in [
        (
            Some("v1"),
            true,
            "legacy-only-marker",
            "v2-only-marker",
            true,
        ),
        (
            Some("v2"),
            true,
            "v2-only-marker",
            "legacy-only-marker",
            true,
        ),
        (None, true, "legacy-only-marker", "v2-only-marker", true),
        (None, false, "v2-only-marker", "legacy-only-marker", true),
        (
            Some("v2"),
            true,
            "v2-only-marker",
            "legacy-only-marker",
            false,
        ),
    ] {
        let server = start_mock_server().await;
        let home = TempDir::new()?;
        MockResponsesConfig::new(&server.uri())
            .enable_feature(Feature::MemoryTool)
            .write(home.path())?;
        for (directory, marker) in [
            ("memories", "legacy-only-marker"),
            ("memories_v2", "v2-only-marker"),
        ] {
            if directory == "memories" && !has_v1_workspace {
                continue;
            }
            tokio::fs::create_dir_all(home.path().join(directory)).await?;
            tokio::fs::write(
                home.path().join(directory).join("memory_summary.md"),
                format!("v1\n{marker}\n"),
            )
            .await?;
        }
        let mock = mount_sse_once(
            &server,
            sse(vec![
                ev_response_created("response"),
                ev_assistant_message("message", "Done"),
                ev_completed("response"),
            ]),
        )
        .await;
        let mut app = TestAppServer::builder()
            .with_codex_home(home.path())
            .build_initialized()
            .await?;
        let mut config = std::collections::HashMap::from([
            ("memories.generate_memories".to_string(), json!(false)),
            ("memories.use_memories".to_string(), json!(use_memories)),
        ]);
        if let Some(version) = version {
            config.insert("memories.version".to_string(), json!(version));
        }
        let start = app
            .send_thread_start_request_with_auto_env(ThreadStartParams {
                // Skip background consolidation so it cannot consume the read test's response.
                ephemeral: Some(true),
                config: Some(config),
                ..Default::default()
            })
            .await?;
        let response: ThreadStartResponse = app.read_response(start).await?;
        let summary_request = app
            .send_raw_request(
                "thread/memory/read",
                Some(json!({ "threadId": response.thread.id })),
            )
            .await?;
        let summary: ThreadMemoryReadResponse = app.read_response(summary_request).await?;
        assert_eq!(
            summary.summary,
            use_memories.then(|| format!("v1\n{expected}\n"))
        );
        assert_eq!(summary.scope.as_deref(), Some("global"));
        app.send_turn_start_request(TurnStartParams {
            thread_id: response.thread.id.clone(),
            input: vec![UserInput::Text {
                text: "Use my memory preferences".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
        let completed: TurnCompletedNotification = app.read_notification("turn/completed").await?;
        assert_eq!(completed.turn.status, TurnStatus::Completed);
        let request = mock.single_request();
        assert_eq!(request.body_contains_text(expected), use_memories);
        assert!(!request.body_contains_text(excluded));

        let directory = if version == Some("v1") || (version.is_none() && has_v1_workspace) {
            "memories"
        } else {
            "memories_v2"
        };
        tokio::fs::remove_file(home.path().join(directory).join("memory_summary.md")).await?;
        let summary_request = app
            .send_raw_request(
                "thread/memory/read",
                Some(json!({ "threadId": response.thread.id })),
            )
            .await?;
        let summary: ThreadMemoryReadResponse = app.read_response(summary_request).await?;
        assert_eq!(summary.summary, None);
    }
    Ok(())
}

fn fixture_git(cwd: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn read_memory(
    app: &mut TestAppServer,
    id: &str,
    include_disabled: bool,
) -> Result<ThreadMemoryReadResponse> {
    let request = app
        .send_raw_request(
            "thread/memory/read",
            Some(json!({ "threadId": id, "includeDisabled": include_disabled })),
        )
        .await?;
    app.read_response(request).await
}

async fn memory_fixture_turn(app: &mut TestAppServer, id: &str) -> Result<()> {
    app.send_turn_start_request(TurnStartParams {
        thread_id: id.to_string(),
        input: vec![UserInput::Text {
            text: "fixture request".to_string(),
            text_elements: Vec::new(),
        }],
        ..Default::default()
    })
    .await?;
    let completed: TurnCompletedNotification = app.read_notification("turn/completed").await?;
    assert_eq!(completed.thread_id, id);
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    Ok(())
}

#[tokio::test]
async fn project_memory_isolates_clones_shares_worktrees_preserves_prefix_and_resume() -> Result<()>
{
    use codex_app_server_protocol::{
        ThreadMemoryNoteAddResponse, ThreadResumeParams, ThreadResumeResponse,
    };
    let server = start_mock_server().await;
    let home = TempDir::new()?;
    let fixture = TempDir::new()?;
    fixture_git(fixture.path(), &["init", "-q", "repo-a"]);
    let repo_a = fixture.path().join("repo-a");
    fixture_git(
        &repo_a,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    );
    let worktree = fixture.path().join("worktree-a");
    fixture_git(
        &repo_a,
        &[
            "worktree",
            "add",
            "-qb",
            "fixture-worktree",
            worktree.to_str().unwrap(),
        ],
    );
    fixture_git(
        fixture.path(),
        &["clone", "-q", repo_a.to_str().unwrap(), "repo-b"],
    );
    let repo_b = fixture.path().join("repo-b");
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::MemoryTool)
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    tokio::fs::create_dir_all(home.path().join("memories_v2")).await?;
    tokio::fs::write(
        home.path().join("memories_v2/memory_summary.md"),
        "global-must-not-be-imported",
    )
    .await?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let mut threads = Vec::new();
    let mut roots = Vec::new();
    for cwd in [&repo_a, &repo_b, &worktree] {
        let request = app
            .send_thread_start_request_with_auto_env(ThreadStartParams {
                cwd: Some(cwd.to_string_lossy().into_owned()),
                config: Some(std::collections::HashMap::from([
                    ("memories.scope".to_string(), json!("project")),
                    ("memories.version".to_string(), json!("v2")),
                    ("memories.generate_memories".to_string(), json!(false)),
                ])),
                ..Default::default()
            })
            .await?;
        let started: ThreadStartResponse = app.read_response(request).await?;
        let memory = read_memory(&mut app, &started.thread.id, true).await?;
        assert_eq!(memory.scope.as_deref(), Some("project"));
        assert_eq!(memory.status.as_deref(), Some("missing"));
        assert!(
            memory.summary.is_none(),
            "project mode must not import global memory"
        );
        roots.push(std::path::PathBuf::from(memory.root.unwrap()));
        threads.push(started.thread.id);
    }
    assert_eq!(roots[0], roots[2]);
    assert_ne!(roots[0], roots[1]);
    for (root, marker) in [
        (&roots[0], "project-a-original"),
        (&roots[1], "project-b-only"),
    ] {
        tokio::fs::create_dir_all(root).await?;
        tokio::fs::write(root.join("memory_summary.md"), marker).await?;
    }
    let mut first_input = Vec::new();
    for index in 0..3 {
        let mock = mount_sse_once(
            &server,
            sse(vec![
                ev_response_created("fixture"),
                ev_assistant_message("message", "done"),
                ev_completed("fixture"),
            ]),
        )
        .await;
        memory_fixture_turn(&mut app, &threads[index]).await?;
        let request = mock.single_request();
        assert!(!request.body_contains_text("global-must-not-be-imported"));
        if index == 0 {
            assert!(request.body_contains_text("project-a-original"));
            assert!(!request.body_contains_text("project-b-only"));
            first_input = request.input();
            tokio::fs::write(roots[0].join("memory_summary.md"), "project-a-updated").await?;
        } else if index == 1 {
            assert!(request.body_contains_text("project-b-only"));
            assert!(!request.body_contains_text("project-a-original"));
            assert!(!request.body_contains_text("project-a-updated"));
        } else {
            assert!(request.body_contains_text("project-a-updated"));
            assert!(!request.body_contains_text("project-b-only"));
        }
    }
    let mock = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("continuation"),
            ev_assistant_message("message2", "done"),
            ev_completed("continuation"),
        ]),
    )
    .await;
    memory_fixture_turn(&mut app, &threads[0]).await?;
    let request = mock.single_request();
    assert!(request.body_contains_text("project-a-original"));
    assert!(!request.body_contains_text("project-a-updated"));
    let continued_input = request.input();
    assert_eq!(
        &continued_input[..first_input.len()],
        first_input.as_slice(),
        "steady-turn model input retains its exact initial prefix"
    );
    assert_eq!(
        read_memory(&mut app, &threads[0], true)
            .await?
            .summary
            .as_deref(),
        Some("project-a-updated")
    );
    let request = app.send_raw_request("thread/memory/note/add", Some(json!({ "threadId": threads[0], "note": "Explicit user correction: retain the new project decision." }))).await?;
    let note: ThreadMemoryNoteAddResponse = app.read_response(request).await?;
    assert!(note.pending_consolidation);
    assert!(roots[0].join("extensions/ad_hoc/notes").is_dir());
    assert!(!roots[1].join("extensions/ad_hoc/notes").exists());
    assert_eq!(
        tokio::fs::read_to_string(roots[0].join("memory_summary.md")).await?,
        "project-a-updated",
        "note ACK does not claim completed consolidation"
    );
    let request = app
        .send_raw_request(
            "thread/memory/note/add",
            Some(json!({ "threadId": threads[0], "note": "x".repeat(16_385) })),
        )
        .await?;
    let error = app
        .read_stream_until_error_message(codex_app_server_protocol::RequestId::Integer(request))
        .await?;
    assert!(error.error.message.contains("16384"));
    app.shutdown_gracefully().await?;
    let mut config = std::fs::read_to_string(home.path().join("config.toml"))?;
    config.push_str("\n[memories]\nscope = \"global\"\nversion = \"v1\"\nuse_memories = false\ngenerate_memories = false\n");
    std::fs::write(home.path().join("config.toml"), config)?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let request = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: threads[0].clone(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = app.read_response(request).await?;
    let disabled = read_memory(&mut app, &threads[0], false).await?;
    assert_eq!(disabled.scope.as_deref(), Some("project"));
    assert_eq!(disabled.status.as_deref(), Some("disabled"));
    assert!(disabled.summary.is_none());
    let inspected = read_memory(&mut app, &threads[0], true).await?;
    assert_eq!(inspected.root.as_deref(), roots[0].to_str());
    assert_eq!(inspected.summary.as_deref(), Some("project-a-updated"));
    assert_eq!(inspected.use_memories, Some(false));
    assert_eq!(inspected.generate_memories, Some(false));
    Ok(())
}

#[tokio::test]
async fn project_opt_in_does_not_rebind_legacy_global_chat_on_resume() -> Result<()> {
    use codex_app_server_protocol::{ThreadResumeParams, ThreadResumeResponse};
    let server = start_mock_server().await;
    let home = TempDir::new()?;
    let repo = TempDir::new()?;
    fixture_git(repo.path(), &["init", "-q"]);
    MockResponsesConfig::new(&server.uri())
        .enable_feature(Feature::MemoryTool)
        .enable_feature(Feature::Sqlite)
        .write(home.path())?;
    tokio::fs::create_dir_all(home.path().join("memories")).await?;
    tokio::fs::write(
        home.path().join("memories/memory_summary.md"),
        "legacy-global-evidence",
    )
    .await?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let request = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            cwd: Some(repo.path().to_string_lossy().into_owned()),
            config: Some(std::collections::HashMap::from([
                ("memories.version".to_string(), json!("v1")),
                ("memories.generate_memories".to_string(), json!(false)),
            ])),
            ..Default::default()
        })
        .await?;
    let started: ThreadStartResponse = app.read_response(request).await?;
    let path = started
        .thread
        .path
        .as_ref()
        .expect("persistent rollout path");
    let _mock = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("legacy"),
            ev_assistant_message("legacy-message", "done"),
            ev_completed("legacy"),
        ]),
    )
    .await;
    memory_fixture_turn(&mut app, &started.thread.id).await?;
    app.shutdown_gracefully().await?;
    // This fixture alone is rewritten to the pre-project wire format.
    let mut removed = 0;
    let mut lines = Vec::new();
    for line in std::fs::read_to_string(path)?.lines() {
        let mut item: serde_json::Value = serde_json::from_str(line)?;
        if let Some(settings) = item
            .get_mut("payload")
            .and_then(|payload| payload.get_mut("thread_settings"))
            .and_then(serde_json::Value::as_object_mut)
        {
            removed += usize::from(settings.remove("memory_scope").is_some());
        }
        lines.push(serde_json::to_string(&item)?);
    }
    assert!(
        removed > 0,
        "the initial scope must be persisted before testing legacy absence"
    );
    std::fs::write(path, lines.join("\n") + "\n")?;
    let mut config = std::fs::read_to_string(home.path().join("config.toml"))?;
    config.push_str(
        "\n[memories]\nscope = \"project\"\nversion = \"v1\"\ngenerate_memories = false\n",
    );
    std::fs::write(home.path().join("config.toml"), config)?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let request = app
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: started.thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let _: ThreadResumeResponse = app.read_response(request).await?;
    let legacy = read_memory(&mut app, &started.thread.id, true).await?;
    assert_eq!(legacy.scope.as_deref(), Some("global"));
    assert_eq!(legacy.summary.as_deref(), Some("legacy-global-evidence"));
    let request = app
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            cwd: Some(repo.path().to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await?;
    let fresh: ThreadStartResponse = app.read_response(request).await?;
    let memory = read_memory(&mut app, &fresh.thread.id, true).await?;
    assert_eq!(memory.scope.as_deref(), Some("project"));
    assert!(memory.summary.is_none());
    assert_ne!(memory.root, legacy.root);
    Ok(())
}
