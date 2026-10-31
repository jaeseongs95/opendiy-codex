use super::*;
use crate::ManagedSubagentAction as Action;
use crate::ManagedSubagentControlOutcome as Outcome;
use crate::ManagedSubagentStatus as Status;
use crate::session::TurnInput;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskResult;

struct OperatorTestTask;

impl SessionTask for OperatorTestTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Regular
    }
    fn span_name(&self) -> &'static str {
        "session_task.operator_test"
    }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation: CancellationToken,
    ) -> SessionTaskResult {
        cancellation.cancelled().await;
        Ok(None)
    }
}

async fn managed_fixture() -> (
    AgentControlHarness,
    ThreadId,
    Arc<CodexThread>,
    LocalAgentControl,
) {
    let (home, mut config) = test_config().await;
    let _ = config.features.enable(Feature::MultiAgentV2);
    let _ = config.features.enable(Feature::Sqlite);
    let harness = AgentControlHarness::new_with_config(home, config).await;
    let (parent_id, parent) = harness.start_thread().await;
    // A read before the first turn resolves support without creating a turn.
    assert!(
        harness
            .manager
            .list_managed_subagents(parent_id)
            .await
            .unwrap()
            .agents
            .is_empty()
    );
    let _ = parent.session.new_default_turn().await;
    let control = parent
        .session
        .services
        .local_agent_runtime
        .control(parent.session.session_id());
    (harness, parent_id, parent, control)
}

#[tokio::test]
async fn managed_operator_fences_identity_preserves_user_input_and_interrupts_exact_turn() {
    let (harness, parent_id, parent, control) = managed_fixture().await;
    let child =
        spawn_v2_reload_test_child(&control, harness.config.clone(), &parent, "worker").await;
    let thread = harness.manager.get_thread(child.thread_id).await.unwrap();
    persist_thread_for_tree_resume(&thread, "synthetic child history").await;
    let turn = thread.session.new_default_turn().await;
    let turn_id = turn.sub_id.clone();
    thread
        .session
        .spawn_task(turn, Vec::new(), OperatorTestTask)
        .await;
    let cancellation = thread
        .session
        .active_turn
        .lock()
        .await
        .as_ref()
        .unwrap()
        .task
        .as_ref()
        .unwrap()
        .cancellation_token
        .clone();
    let parent_runtime = parent.runtime_id();
    let child_runtime = thread.runtime_id();

    for (expected_parent, expected_child) in [
        (Uuid::new_v4(), Some(child_runtime)),
        (parent_runtime, Some(Uuid::new_v4())),
        (parent_runtime, None),
    ] {
        let result = harness
            .manager
            .control_managed_subagent(
                parent_id,
                expected_parent,
                child.thread_id,
                expected_child,
                Action::Interrupt {
                    expected_turn_id: turn_id.clone(),
                },
            )
            .await
            .unwrap();
        assert_eq!(result, Outcome::NotApplied);
        assert!(!cancellation.is_cancelled());
    }
    assert_eq!(
        harness
            .manager
            .control_managed_subagent(
                parent_id,
                parent_runtime,
                child.thread_id,
                Some(child_runtime),
                Action::Interrupt {
                    expected_turn_id: "old-turn".into()
                }
            )
            .await
            .unwrap(),
        Outcome::NotApplied
    );
    assert!(!cancellation.is_cancelled());

    let content = text_input("synthetic operator instruction");
    assert_eq!(
        harness
            .manager
            .control_managed_subagent(
                parent_id,
                parent_runtime,
                child.thread_id,
                Some(child_runtime),
                Action::Send {
                    input: content.clone()
                }
            )
            .await
            .unwrap(),
        Outcome::Applied
    );
    let pending = thread
        .session
        .input_queue
        .get_pending_input(&thread.session.active_turn)
        .await
        .0;
    assert!(
        matches!(pending.as_slice(), [TurnInput::UserInput { content: actual, metadata, .. }]
        if actual == &content && metadata.origin == codex_history::UserInputOrigin::User)
    );

    let listed = harness
        .manager
        .list_managed_subagents(parent_id)
        .await
        .unwrap();
    assert_eq!(listed.parent_runtime_id, parent_runtime);
    let entry = listed
        .agents
        .iter()
        .find(|entry| entry.thread_id == child.thread_id)
        .unwrap();
    assert_eq!(entry.active_turn_id.as_deref(), Some(turn_id.as_str()));
    assert!(entry.capabilities.interrupt);
    assert_eq!(
        harness
            .manager
            .control_managed_subagent(
                parent_id,
                parent_runtime,
                child.thread_id,
                Some(child_runtime),
                Action::Interrupt {
                    expected_turn_id: turn_id.clone()
                }
            )
            .await
            .unwrap(),
        Outcome::Applied
    );
    assert!(cancellation.is_cancelled());
    timeout(Duration::from_secs(5), async {
        loop {
            if matches!(thread.next_event().await.unwrap().msg,
                EventMsg::TurnAborted(TurnAbortedEvent { turn_id: Some(ref id), reason: TurnAbortReason::Interrupted, .. })
                if id == &turn_id) { break }
        }
    }).await.expect("exact turn emits the normal interrupted lifecycle event");
    assert!(thread.session.active_turn.lock().await.is_none());
    let _ = harness
        .manager
        .shutdown_all_threads_bounded(Duration::from_secs(5))
        .await;
}

#[tokio::test]
async fn managed_close_keeps_closed_metadata_closes_descendants_and_resume_changes_runtime() {
    let (harness, parent_id, parent, control) = managed_fixture().await;
    let child =
        spawn_v2_reload_test_child(&control, harness.config.clone(), &parent, "worker").await;
    let thread = harness.manager.get_thread(child.thread_id).await.unwrap();
    let grandchild =
        spawn_v2_reload_test_child(&control, harness.config.clone(), &thread, "nested").await;
    let nested = harness
        .manager
        .get_thread(grandchild.thread_id)
        .await
        .unwrap();
    persist_thread_for_tree_resume(&thread, "synthetic child context").await;
    persist_thread_for_tree_resume(&nested, "synthetic nested context").await;
    let turn = thread.session.new_default_turn().await;
    thread
        .session
        .spawn_task(turn, Vec::new(), OperatorTestTask)
        .await;
    control
        .send_inter_agent_communication(
            child.thread_id,
            InterAgentCommunication::new(
                AgentPath::root(),
                child.metadata.agent_path.clone().unwrap(),
                Vec::new(),
                "synthetic queued mailbox message".into(),
                false,
            ),
            AgentCommunicationContext::new(AgentCommunicationKind::Message, parent_id),
            Default::default(),
        )
        .await
        .unwrap();
    let held_mailbox_pin = Arc::clone(&thread.residency_gate).read_owned().await;
    let parent_runtime = parent.runtime_id();
    let old_runtime = thread.runtime_id();
    let listed = harness
        .manager
        .list_managed_subagents(parent_id)
        .await
        .unwrap();
    assert_eq!(listed.agents.len(), 1);
    assert_eq!(listed.agents[0].thread_id, child.thread_id);
    assert_eq!(
        harness
            .manager
            .control_managed_subagent(
                parent_id,
                parent_runtime,
                grandchild.thread_id,
                Some(nested.runtime_id()),
                Action::Close
            )
            .await
            .unwrap(),
        Outcome::NotApplied
    );
    assert_eq!(
        timeout(
            Duration::from_secs(5),
            harness.manager.control_managed_subagent(
                parent_id,
                parent_runtime,
                child.thread_id,
                Some(old_runtime),
                Action::Close
            )
        )
        .await
        .expect("close must not wait for mailbox residency pins")
        .unwrap(),
        Outcome::Applied
    );
    drop(held_mailbox_pin);
    assert_thread_not_loaded(&harness.manager, child.thread_id).await;
    assert_thread_not_loaded(&harness.manager, grandchild.thread_id).await;
    let listed = harness
        .manager
        .list_managed_subagents(parent_id)
        .await
        .unwrap();
    assert_eq!(listed.agents.len(), 1);
    assert_eq!(listed.agents[0].runtime_id, None);
    assert_eq!(listed.agents[0].status, Status::Closed);
    assert!(listed.agents[0].capabilities.resume);
    assert_eq!(
        harness
            .manager
            .control_managed_subagent(
                parent_id,
                parent_runtime,
                child.thread_id,
                Some(old_runtime),
                Action::Resume
            )
            .await
            .unwrap(),
        Outcome::NotApplied
    );
    assert_eq!(
        harness
            .manager
            .control_managed_subagent(
                parent_id,
                parent_runtime,
                child.thread_id,
                None,
                Action::Resume
            )
            .await
            .unwrap(),
        Outcome::Applied
    );
    let resumed = harness.manager.get_thread(child.thread_id).await.unwrap();
    assert_ne!(old_runtime, resumed.runtime_id());
    assert_eq!(
        harness
            .manager
            .control_managed_subagent(
                parent_id,
                parent_runtime,
                child.thread_id,
                Some(old_runtime),
                Action::Close
            )
            .await
            .unwrap(),
        Outcome::NotApplied
    );
    assert!(resumed.is_running());
    let _ = harness
        .manager
        .shutdown_all_threads_bounded(Duration::from_secs(5))
        .await;
}

#[tokio::test]
async fn managed_legacy_parent_is_explicitly_unsupported() {
    let (home, mut config) = test_config().await;
    config.agents_enabled = false;
    let harness = AgentControlHarness::new_with_config(home, config).await;
    let (parent_id, _) = harness.start_thread().await;
    let error = harness
        .manager
        .list_managed_subagents(parent_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error.details(),
        CodexErrorDetails::UnsupportedOperation(_)
    ));
    let _ = harness
        .manager
        .shutdown_all_threads_bounded(Duration::from_secs(5))
        .await;
}

#[test]
fn managed_metadata_limit_rejects_an_incomplete_snapshot() {
    let registry = Arc::new(crate::agent::registry::AgentRegistry::default());
    let parent = ThreadId::new();
    registry.register_root_thread(parent);
    for index in 0..1025 {
        let path = AgentPath::root().join(&format!("child_{index}")).unwrap();
        let mut reservation = registry.reserve_spawn_slot(None).unwrap();
        reservation.reserve_agent_path(&path).unwrap();
        reservation.commit(crate::AgentMetadata {
            agent_id: Some(ThreadId::new()),
            agent_path: Some(path),
            ..Default::default()
        });
        if index == 1023 {
            assert_eq!(registry.managed_children(parent).unwrap().len(), 1024);
        }
    }
    assert!(registry.managed_children(parent).is_err());
}
