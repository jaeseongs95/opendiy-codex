// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
use super::*;
use pretty_assertions::assert_eq;

fn runtime_tool(name: &str, description: &str) -> DynamicToolSpec {
    DynamicToolSpec::Function(codex_protocol::dynamic_tools::DynamicToolFunctionSpec {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object", "properties":{}, "additionalProperties":false}),
        defer_loading: false,
    })
}

#[tokio::test]
async fn runtime_tools_preserve_custom_storage_inheritance_and_disconnect_admission() {
    let original = vec![runtime_tool("custom", "user-owned")];
    let (mut session, _, _) =
        make_session_and_context_with_dynamic_tools_and_rx(original.clone()).await;
    let path = attach_thread_persistence(Arc::get_mut(&mut session).expect("unique session")).await;
    let closed = CancellationToken::new();
    let supplement = vec![runtime_tool("local_control", "current host")];
    assert!(
        session
            .supplement_runtime_dynamic_tools(7, closed.clone(), supplement.clone(), &[])
            .await
            .unwrap()
    );
    assert_eq!(
        session.dynamic_tools().await.as_ref(),
        &[original[0].clone(), supplement[0].clone()]
    );
    assert_eq!(
        session.inheritable_dynamic_tools().await.as_ref(),
        original.as_slice()
    );
    assert_eq!(
        session.state.lock().await.persisted_dynamic_tools.as_ref(),
        original.as_slice()
    );
    session.flush_rollout().await.unwrap();
    let history = RolloutRecorder::get_rollout_history(&path).await.unwrap();
    assert_eq!(history.get_dynamic_tools(), Some(original.clone()));
    assert!(
        !history
            .get_rollout_items()
            .iter()
            .any(|item| matches!(item, RolloutItem::DynamicToolsUpdated { .. }))
    );
    assert!(
        session
            .supplement_runtime_dynamic_tools(8, CancellationToken::new(), supplement.clone(), &[])
            .await
            .is_err()
    );
    *session.active_turn.lock().await = Some(ActiveTurn::default());
    assert!(
        !session
            .supplement_runtime_dynamic_tools(7, closed.clone(), Vec::new(), &[])
            .await
            .unwrap()
    );
    closed.cancel();
    let (_, route) = session
        .runtime_dynamic_tool_owner(None, "local_control")
        .await
        .unwrap();
    assert!(route.is_cancelled());
    // An in-flight catalog remains stable; only the next idle admission removes it.
    assert_eq!(session.dynamic_tools().await.len(), 2);
    *session.active_turn.lock().await = None;
    session.reserve_compact_if_idle().await.unwrap().unwrap();
    assert_eq!(session.dynamic_tools().await.as_ref(), original.as_slice());
    assert!(
        session
            .runtime_dynamic_tool_owner(None, "local_control")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn runtime_tools_reject_unknown_collision_without_partial_publication() {
    let old = runtime_tool("local_control", "known previous host");
    let replacement = runtime_tool("local_control", "current host");
    let original = vec![runtime_tool("custom", "user-owned"), old.clone()];
    let (session, _, _) =
        make_session_and_context_with_dynamic_tools_and_rx(original.clone()).await;
    let closed = CancellationToken::new();
    assert!(
        session
            .supplement_runtime_dynamic_tools(
                7,
                closed.clone(),
                vec![
                    runtime_tool("new_tool", "must not leak on conflict"),
                    replacement.clone()
                ],
                &[]
            )
            .await
            .is_err()
    );
    assert_eq!(session.dynamic_tools().await.as_ref(), original.as_slice());
    assert!(
        session
            .supplement_runtime_dynamic_tools(7, closed.clone(), vec![replacement.clone()], &[old])
            .await
            .unwrap()
    );
    assert_eq!(
        session.dynamic_tools().await.as_ref(),
        &[original[0].clone(), replacement]
    );
    assert_eq!(
        session.inheritable_dynamic_tools().await.as_ref(),
        original.as_slice()
    );
    closed.cancel();
    assert!(
        session
            .supplement_runtime_dynamic_tools(7, closed, Vec::new(), &[])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn explicit_tool_replacement_supersedes_runtime_catalog_without_persisting_overlay() {
    let (session, _, _) = make_session_and_context_with_dynamic_tools_and_rx(vec![runtime_tool(
        "old_custom",
        "saved",
    )])
    .await;
    assert!(
        session
            .supplement_runtime_dynamic_tools(
                7,
                CancellationToken::new(),
                vec![runtime_tool("connection_only", "host")],
                &[]
            )
            .await
            .unwrap()
    );
    let replacement = vec![runtime_tool("new_custom", "replacement")];
    *session.active_turn.lock().await = Some(ActiveTurn::default());
    assert!(
        session
            .replace_dynamic_tools(replacement.clone())
            .await
            .is_err()
    );
    assert!(
        session
            .runtime_dynamic_tool_owner(None, "connection_only")
            .await
            .is_some()
    );
    *session.active_turn.lock().await = None;
    session
        .replace_dynamic_tools(replacement.clone())
        .await
        .unwrap();
    assert_eq!(
        session.dynamic_tools().await.as_ref(),
        replacement.as_slice()
    );
    assert_eq!(
        session.inheritable_dynamic_tools().await.as_ref(),
        replacement.as_slice()
    );
    assert_eq!(
        session.state.lock().await.persisted_dynamic_tools.as_ref(),
        replacement.as_slice()
    );
    assert!(
        session
            .runtime_dynamic_tool_owner(None, "connection_only")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn dynamic_tools_updates_preserve_submission_order_after_caller_stops_waiting() {
    let (mut session, turn_context, rx_event) =
        make_session_and_context_with_auth_and_config_and_rx(
            CodexAuth::from_api_key("Test API Key"),
            Vec::new(),
            |config| config.allow_persistent_dynamic_tool_updates = true,
        )
        .await;
    let rollout_path =
        attach_thread_persistence(Arc::get_mut(&mut session).expect("unique session")).await;
    let (thread, rx_sub) = make_thread_for_session(
        Arc::clone(&session),
        &turn_context,
        rx_event,
        SessionSource::Exec,
    )
    .await;
    let replacement = vec![DynamicToolSpec::Function(
        codex_protocol::dynamic_tools::DynamicToolFunctionSpec {
            name: "check_release".into(),
            description: "Check the release.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: false,
        },
    )];
    let persistence = thread_settings::acquire_persistence_lock(&session).await;
    let mut first = Box::pin(tokio::task::unconstrained(
        thread.replace_dynamic_tools(Vec::new()),
    ));
    assert!(futures::poll!(first.as_mut()).is_pending());
    drop(first);
    let mut submissions = Box::pin(tokio::task::unconstrained(submission_loop(
        Arc::clone(&session),
        Arc::clone(&turn_context.config),
        rx_sub,
    )));
    assert!(futures::poll!(submissions.as_mut()).is_pending());
    let mut last = Box::pin(tokio::task::unconstrained(
        thread.replace_dynamic_tools(replacement.clone()),
    ));
    assert!(futures::poll!(last.as_mut()).is_pending());
    let mut shutdown = Box::pin(tokio::task::unconstrained(thread.submit(Op::Shutdown)));
    // Shutdown cannot overtake the accepted replacements while persistence is blocked.
    assert!(futures::poll!(shutdown.as_mut()).is_pending());
    assert_eq!(session.dynamic_tools().await.as_ref(), &[]);
    drop(persistence);
    let ((), updated, stopped) = tokio::join!(submissions, last, shutdown);
    updated.expect("replace dynamic tools before shutdown");
    stopped.expect("submit shutdown");
    assert_eq!(
        session.dynamic_tools().await.as_ref(),
        replacement.as_slice()
    );
    let restored = RolloutRecorder::get_rollout_history(&rollout_path)
        .await
        .expect("read persisted catalog");
    let catalogs: Vec<_> = restored
        .get_rollout_items()
        .iter()
        .filter_map(|item| match item {
            RolloutItem::DynamicToolsUpdated { tools, .. } => Some(tools.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(catalogs, vec![Vec::new(), replacement.clone()]);
    assert_eq!(restored.get_dynamic_tools(), Some(replacement));
    assert!(matches!(
        thread
            .replace_dynamic_tools(Vec::new())
            .await
            .unwrap_err()
            .details(),
        codex_protocol::error::CodexErrorDetails::InternalAgentDied
    ));
}

#[tokio::test]
async fn dynamic_tools_update_requires_host_opt_in_only_for_persistent_threads() {
    let (mut session, _) = make_session_and_context().await;
    let initial_tools = vec![DynamicToolSpec::Function(
        codex_protocol::dynamic_tools::DynamicToolFunctionSpec {
            name: "check_release".into(),
            description: "Check the release.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: false,
        },
    )];
    session
        .replace_dynamic_tools(initial_tools.clone())
        .await
        .unwrap();
    let path = attach_thread_persistence(&mut session).await;
    let error = session.replace_dynamic_tools(Vec::new()).await.unwrap_err();
    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::UnsupportedOperation(_)
    ));
    assert_eq!(
        session.dynamic_tools().await.as_ref(),
        initial_tools.as_slice()
    );
    session.flush_rollout().await.unwrap();
    let history = RolloutRecorder::get_rollout_history(&path).await.unwrap();
    assert_eq!(history.get_dynamic_tools(), Some(initial_tools));
}

#[tokio::test]
async fn dynamic_tools_update_keeps_catalog_when_persistence_fails() {
    let (mut session, _, _rx) = make_session_and_context_with_auth_and_config_and_rx(
        CodexAuth::from_api_key("Test API Key"),
        Vec::new(),
        |config| config.allow_persistent_dynamic_tool_updates = true,
    )
    .await;
    attach_thread_persistence(Arc::get_mut(&mut session).expect("unique session")).await;
    session
        .live_thread()
        .expect("live thread")
        .shutdown()
        .await
        .expect("close persistence");
    let replacement = vec![codex_protocol::dynamic_tools::DynamicToolSpec::Function(
        codex_protocol::dynamic_tools::DynamicToolFunctionSpec {
            name: "check_release".into(),
            description: "Check the release.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
            defer_loading: false,
        },
    )];
    let error = session
        .replace_dynamic_tools(replacement)
        .await
        .unwrap_err();
    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::Fatal(_)
    ));
    assert_eq!(session.dynamic_tools().await.as_ref(), &[]);
}
