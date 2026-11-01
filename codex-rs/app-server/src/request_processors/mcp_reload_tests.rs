// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
use super::*;
use codex_config::ThreadConfigContext;
use codex_config::ThreadConfigLoadError;
use codex_config::ThreadConfigLoader;
use codex_config::ThreadConfigSource;
use codex_exec_server::EnvironmentManager;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InternalSessionSource;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::ThreadHistoryMode;
use pretty_assertions::assert_eq;

#[derive(Default)]
struct ReloadLoader {
    unload: std::sync::Mutex<Option<(Arc<ThreadManager>, ThreadId, bool)>>,
}

impl ThreadConfigLoader for ReloadLoader {
    fn load(
        &self,
        context: ThreadConfigContext,
    ) -> codex_config::ThreadConfigLoaderFuture<'_, Vec<ThreadConfigSource>> {
        Box::pin(async move {
            let unload = if context.cwd.is_some() {
                self.unload.lock().unwrap().take()
            } else {
                None
            };
            if let Some((threads, id, replace)) = unload {
                let old = threads.remove_thread(&id).await.unwrap();
                if replace {
                    old.ensure_rollout_materialized().await;
                    old.flush_rollout().await.unwrap();
                    let history = old.load_history(true).await.unwrap();
                    old.shutdown_and_wait().await.unwrap();
                    let replacement = threads
                        .resume_thread_with_history(
                            old.config().await.as_ref().clone(),
                            InitialHistory::Resumed(ResumedHistory {
                                conversation_id: id,
                                history: Arc::new(history.items),
                                history_revision: history.revision,
                                rollout_path: old.rollout_path(),
                            }),
                            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy")),
                            None,
                            Default::default(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(replacement.thread_id, id);
                    assert!(!Arc::ptr_eq(&old, &replacement.thread));
                }
            }
            Ok::<_, ThreadConfigLoadError>(Vec::new())
        })
    }
}

struct Fixture {
    home: tempfile::TempDir,
    processor: McpRequestProcessor,
    threads: Arc<ThreadManager>,
    config: Arc<Config>,
    loader: Arc<ReloadLoader>,
    _receiver: tokio::sync::mpsc::Receiver<crate::outgoing_message::OutgoingEnvelope>,
}

impl Fixture {
    async fn new() -> anyhow::Result<Self> {
        let home = tempfile::tempdir()?;
        let loader = Arc::new(ReloadLoader::default());
        let manager = ConfigManager::new(
            home.path().to_path_buf(),
            Vec::new(),
            codex_config::LoaderOverrides::without_managed_config_for_tests(),
            /*strict_config*/ false,
            codex_config::CloudConfigBundleLoader::default(),
            codex_arg0::Arg0DispatchPaths::default(),
            loader.clone(),
        );
        let config = Arc::new(
            manager
                .load_latest_config(Some(home.path().to_path_buf()))
                .await?,
        );
        let auth = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy"));
        let threads = Arc::new(
            codex_core::test_support::thread_manager_with_models_provider_and_home(
                CodexAuth::from_api_key("dummy"),
                config.model_provider.clone(),
                config.codex_home.to_path_buf(),
                Arc::new(EnvironmentManager::default_for_tests()),
            ),
        );
        let (sender, receiver) = tokio::sync::mpsc::channel(16);
        let outgoing = Arc::new(OutgoingMessageSender::new(
            sender,
            codex_analytics::AnalyticsEventsClient::disabled(),
        ));
        let catalog = Arc::new(crate::model_catalog::ModelCatalog::new(
            manager.clone(),
            Some(config.cwd.to_path_buf()),
            Arc::clone(&config),
            threads.get_models_manager(),
            Arc::clone(&auth),
        ));
        let account = AccountRequestProcessor::new(
            Arc::clone(&auth),
            Arc::clone(&threads),
            Arc::clone(&outgoing),
            Arc::clone(&config),
            manager.clone(),
            catalog,
        );
        Ok(Self {
            home,
            processor: McpRequestProcessor::new(
                auth,
                Arc::clone(&threads),
                ThreadStateManager::new(),
                outgoing,
                manager,
                account,
            ),
            threads,
            config,
            loader,
            _receiver: receiver,
        })
    }

    async fn thread(&self) -> anyhow::Result<(ThreadId, Arc<CodexThread>)> {
        let new = self
            .threads
            .start_thread(codex_core::StartThreadOptions::new(
                self.config.as_ref().clone(),
            ))
            .await?;
        Ok((new.thread_id, new.thread))
    }

    fn invalid_config(&self) -> anyhow::Result<()> {
        std::fs::write(
            self.home.path().join(codex_config::CONFIG_TOML_FILE),
            "invalid = [",
        )?;
        Ok(())
    }
}

fn target(id: ThreadId) -> Option<McpServerRefreshParams> {
    Some(McpServerRefreshParams {
        thread_id: Some(id.to_string()),
    })
}

#[tokio::test]
async fn mcp_reload_handler_preserves_legacy_and_targets_one_loaded_thread() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, _) = fixture.thread().await?;
    let (other_id, other) = fixture.thread().await?;
    for params in [None, Some(McpServerRefreshParams::default())] {
        let legacy = fixture
            .processor
            .mcp_server_refresh_response(params)
            .await
            .expect("legacy reload should succeed");
        assert_eq!(serde_json::to_value(legacy)?, serde_json::json!({}));
    }
    let response = fixture
        .processor
        .mcp_server_refresh_response(target(id))
        .await
        .expect("targeted reload should succeed");
    assert_eq!(
        response.reconnect_queued_for_thread_id,
        Some(id.to_string())
    );
    assert!(Arc::ptr_eq(
        &other,
        &fixture.threads.get_thread(other_id).await?
    ));
    Ok(())
}

#[tokio::test]
async fn mcp_reload_handler_rejects_internal_target_before_policy_load() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (parent_id, _) = fixture.thread().await?;
    let internal = fixture
        .threads
        .spawn_internal_session(
            parent_id,
            codex_core::StartThreadOptions {
                session_source: Some(SessionSource::Internal(InternalSessionSource::Guardian)),
                ..codex_core::StartThreadOptions::new(fixture.config.as_ref().clone())
            },
        )
        .await?;
    fixture.invalid_config()?;
    let error = fixture
        .processor
        .mcp_server_refresh_response(target(internal.thread_id))
        .await
        .unwrap_err();
    assert_eq!(
        error.message,
        format!("thread not found: {}", internal.thread_id)
    );
    Ok(())
}

#[tokio::test]
async fn mcp_reload_handler_rejects_missing_or_invalid_target_before_policy_load()
-> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    fixture.invalid_config()?;
    for id in ["not-a-thread".to_string(), ThreadId::new().to_string()] {
        let error = fixture
            .processor
            .mcp_server_refresh_response(Some(McpServerRefreshParams {
                thread_id: Some(id),
            }))
            .await
            .unwrap_err();
        assert!(
            error.message.starts_with("invalid thread id:")
                || error.message.starts_with("thread not found:")
        );
    }
    Ok(())
}

#[tokio::test]
async fn mcp_reload_handler_policy_failure_has_no_reconnect_ack() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, _) = fixture.thread().await?;
    fixture.invalid_config()?;
    let error = fixture
        .processor
        .mcp_server_refresh_response(target(id))
        .await
        .unwrap_err();
    assert!(error.message.starts_with("failed to refresh MCP servers:"));
    Ok(())
}

#[tokio::test]
async fn mcp_reload_handler_rechecks_loaded_owner_after_policy_reload() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, _) = fixture.thread().await?;
    *fixture.loader.unload.lock().unwrap() = Some((Arc::clone(&fixture.threads), id, false));
    let error = fixture
        .processor
        .mcp_server_refresh_response(target(id))
        .await
        .unwrap_err();
    assert_eq!(error.message, format!("thread is no longer loaded: {id}"));
    Ok(())
}

#[tokio::test]
async fn mcp_reload_handler_rejects_replacement_runtime_with_same_id() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    // This fixture captures full history to resume the same ID under a new runtime.
    let original = fixture
        .threads
        .start_thread(codex_core::StartThreadOptions {
            history_mode: Some(ThreadHistoryMode::Legacy),
            ..codex_core::StartThreadOptions::new(fixture.config.as_ref().clone())
        })
        .await?;
    let (id, old) = (original.thread_id, original.thread);
    *fixture.loader.unload.lock().unwrap() = Some((Arc::clone(&fixture.threads), id, true));
    let error = fixture
        .processor
        .mcp_server_refresh_response(target(id))
        .await
        .unwrap_err();
    assert_eq!(
        error.message,
        format!("thread runtime changed during MCP reload: {id}")
    );
    assert!(!Arc::ptr_eq(&old, &fixture.threads.get_thread(id).await?));
    Ok(())
}

#[tokio::test]
async fn mcp_reload_handler_acknowledges_captured_queue_after_registry_removal()
-> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, old) = fixture.thread().await?;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    *fixture.processor.before_refresh_enqueue.lock().unwrap() = Some(Arc::clone(&barrier));
    let processor = fixture.processor.clone();
    let refresh =
        tokio::spawn(async move { processor.mcp_server_refresh_response(target(id)).await });
    let timeout = std::time::Duration::from_secs(10);

    // First rendezvous is after the registry identity check; the second releases enqueue.
    tokio::time::timeout(timeout, barrier.wait()).await?;
    let removed = fixture.threads.remove_thread(&id).await.unwrap();
    assert!(Arc::ptr_eq(&old, &removed));
    assert!(fixture.threads.get_thread(id).await.is_err());
    tokio::time::timeout(timeout, barrier.wait()).await?;

    let response = tokio::time::timeout(timeout, refresh)
        .await??
        .expect("the captured runtime queue remains open");
    assert_eq!(
        response.reconnect_queued_for_thread_id,
        Some(id.to_string())
    );
    assert!(fixture.threads.get_thread(id).await.is_err());
    old.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn mcp_reload_handler_closed_submission_queue_has_no_ack() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, thread) = fixture.thread().await?;
    thread.submit(Op::Shutdown).await?;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !matches!(thread.next_event().await?.msg, EventMsg::ShutdownComplete) {}
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let error = fixture
        .processor
        .mcp_server_refresh_response(target(id))
        .await
        .unwrap_err();
    assert!(
        error
            .message
            .starts_with("failed to queue MCP reconnect for")
    );
    Ok(())
}

fn plugin_target(id: ThreadId) -> codex_app_server_protocol::ThreadPluginReloadParams {
    codex_app_server_protocol::ThreadPluginReloadParams {
        thread_id: id.to_string(),
    }
}

#[tokio::test]
async fn plugin_reload_publishes_only_target_configuration_without_writing_config()
-> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, thread) = fixture.thread().await?;
    let (_, other) = fixture.thread().await?;
    let before = thread.config().await;
    let other_before = other.config().await;
    let file = fixture.home.path().join(codex_config::CONFIG_TOML_FILE);
    let contents = "[apps.calendar]\nenabled = false\n";
    std::fs::write(&file, contents)?;
    let response = fixture
        .processor
        .thread_plugin_reload_response(plugin_target(id))
        .await
        .expect("target plugin reload succeeds");
    assert_eq!(response.thread_id, id.to_string());
    assert!(response.mcp_reconnect_requested);
    assert!(!Arc::ptr_eq(&before, &thread.config().await));
    assert_eq!(
        thread.config().await.config_layer_stack.effective_config()["apps"]["calendar"]["enabled"]
            .as_bool(),
        Some(false)
    );
    // This proves no configuration publication on B. Plugin/skill discovery caches are shared:
    // B's later discovery may see changed files; complete future-capture isolation is not promised.
    assert!(Arc::ptr_eq(&other_before, &other.config().await));
    assert_eq!(std::fs::read_to_string(file)?, contents);
    Ok(())
}

#[tokio::test]
async fn plugin_reload_invalid_config_returns_error_and_keeps_runtime() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, thread) = fixture.thread().await?;
    let before = thread.config().await;
    fixture.invalid_config()?;
    let error = fixture
        .processor
        .thread_plugin_reload_response(plugin_target(id))
        .await
        .unwrap_err();
    assert!(error.message.contains("failed to parse user config"));
    assert!(Arc::ptr_eq(&before, &thread.config().await));
    Ok(())
}

#[tokio::test]
async fn plugin_reload_unloaded_target_never_retargets_visible_thread() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, thread) = fixture.thread().await?;
    let (_, other) = fixture.thread().await?;
    let other_before = other.config().await;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    *fixture.processor.before_plugin_reload.lock().unwrap() = Some(Arc::clone(&barrier));
    let processor = fixture.processor.clone();
    let reload = tokio::spawn(async move {
        processor
            .thread_plugin_reload_response(plugin_target(id))
            .await
    });
    let timeout = std::time::Duration::from_secs(10);
    tokio::time::timeout(timeout, barrier.wait()).await?;
    let removed = fixture.threads.remove_thread(&id).await.unwrap();
    assert!(Arc::ptr_eq(&removed, &thread));
    tokio::time::timeout(timeout, barrier.wait()).await?;
    let error = tokio::time::timeout(timeout, reload).await??.unwrap_err();
    assert_eq!(error.message, format!("thread is no longer loaded: {id}"));
    assert!(Arc::ptr_eq(&other_before, &other.config().await));
    thread.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test]
async fn plugin_reload_closed_runtime_has_no_success_ack() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let (id, thread) = fixture.thread().await?;
    thread.shutdown_and_wait().await?;
    let error = fixture
        .processor
        .thread_plugin_reload_response(plugin_target(id))
        .await
        .unwrap_err();
    assert!(error.message.contains("shutting down"));
    Ok(())
}

#[tokio::test]
async fn plugin_reload_does_not_retarget_replacement_with_same_id() -> anyhow::Result<()> {
    let fixture = Fixture::new().await?;
    let original = fixture
        .threads
        .start_thread(codex_core::StartThreadOptions {
            history_mode: Some(ThreadHistoryMode::Legacy),
            ..codex_core::StartThreadOptions::new(fixture.config.as_ref().clone())
        })
        .await?;
    let (id, old) = (original.thread_id, original.thread);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    *fixture.processor.before_plugin_reload.lock().unwrap() = Some(Arc::clone(&barrier));
    let processor = fixture.processor.clone();
    let reload = tokio::spawn(async move {
        processor
            .thread_plugin_reload_response(plugin_target(id))
            .await
    });
    let timeout = std::time::Duration::from_secs(10);
    tokio::time::timeout(timeout, barrier.wait()).await?;
    fixture.threads.remove_thread(&id).await.unwrap();
    old.ensure_rollout_materialized().await;
    old.flush_rollout().await?;
    let history = old.load_history(true).await?;
    old.shutdown_and_wait().await?;
    let replacement = fixture
        .threads
        .resume_thread_with_history(
            old.config().await.as_ref().clone(),
            InitialHistory::Resumed(ResumedHistory {
                conversation_id: id,
                history: Arc::new(history.items),
                history_revision: history.revision,
                rollout_path: old.rollout_path(),
            }),
            AuthManager::from_auth_for_testing(CodexAuth::from_api_key("dummy")),
            None,
            Default::default(),
        )
        .await?;
    assert_eq!(replacement.thread_id, id);
    let before = replacement.thread.config().await;
    tokio::time::timeout(timeout, barrier.wait()).await?;
    assert!(tokio::time::timeout(timeout, reload).await??.is_err());
    assert!(Arc::ptr_eq(&before, &replacement.thread.config().await));
    Ok(())
}
