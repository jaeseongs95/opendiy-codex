use super::ResponsesStreamRequest;
use super::ResponsesStreamRetryState;
use super::handle_response_stream_error;
use super::log_retry;
use crate::realtime_history::RealtimeHistoryState;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing_test::internal::MockWriter;

#[tokio::test]
async fn capacity_retry_without_hint_and_zero_hint_use_bounded_backoff() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState::for_sampling(
        session.services.model_client.auth_owner_generation(),
    );
    tokio::time::pause();
    for attempt in 1..=5 {
        let before = Instant::now();
        let error = if attempt == 2 {
            CodexErr::ServerOverloaded
                .with_retry_after(RetryAfter::from_delay(Duration::ZERO).expect("valid zero hint"))
        } else {
            CodexErr::ServerOverloaded
        };
        handle_response_stream_error(
            &mut state,
            100,
            error,
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        )
        .await
        .expect("capacity retry within budget");
        let elapsed = Instant::now() - before;
        let base = [5_000, 10_000, 20_000, 40_000, 60_000][attempt - 1];
        assert!(elapsed >= Duration::from_millis(base * 9 / 10));
        // Tokio may round the selected deadline up by one timer tick.
        assert!(
            elapsed
                <= Duration::from_millis(base * 11 / 10).min(Duration::from_secs(60))
                    + Duration::from_millis(1)
        );
    }
    let before = Instant::now();
    let error = handle_response_stream_error(
        &mut state,
        100,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .await
    .expect_err("sixth retry must be rejected");
    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::ServerOverloaded
    ));
    assert_eq!(Instant::now(), before);
    assert_eq!(state.capacity_retries, 5);
}

#[tokio::test]
async fn capacity_retry_respects_provider_zero_and_server_deadline_window() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    tokio::time::pause();
    for (max_retries, hint) in [(0, 0), (5, 180), (5, 181)] {
        let mut state = ResponsesStreamRetryState::for_sampling(
            session.services.model_client.auth_owner_generation(),
        );
        let before = Instant::now();
        handle_response_stream_error(
            &mut state,
            max_retries,
            CodexErr::ServerOverloaded
                .with_retry_after(RetryAfter::from_delay(Duration::from_secs(hint)).expect("hint")),
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        )
        .await
        .expect_err("must preserve overload without an early retry");
        assert_eq!(Instant::now(), before);
        assert_eq!(state.capacity_retries, 0);
    }
}

#[tokio::test]
async fn capacity_retry_honors_later_server_hint_and_cannot_reset_after_fallback() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState::for_sampling(
        session.services.model_client.auth_owner_generation(),
    );
    tokio::time::pause();
    let before = Instant::now();
    handle_response_stream_error(
        &mut state,
        5,
        CodexErr::ServerOverloaded
            .with_retry_after(RetryAfter::from_delay(Duration::from_secs(30)).expect("hint")),
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .await
    .expect("retry");
    assert!(
        (Duration::from_secs(30)..=Duration::from_millis(30_001))
            .contains(&(Instant::now() - before))
    );
    // Model the generic transport fallback's reset; the capacity budget survives it.
    state.retries = 0;
    state.capacity_retries = 5;
    handle_response_stream_error(
        &mut state,
        100,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .await
    .expect_err("transport reset must not grant capacity retries");
}

#[tokio::test]
async fn capacity_retry_is_cancellable_and_compaction_remains_terminal() {
    use codex_async_utils::OrCancelExt;
    use tokio_util::sync::CancellationToken;
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState::for_sampling(
        session.services.model_client.auth_owner_generation(),
    );
    let token = CancellationToken::new();
    tokio::time::pause();
    let retry = handle_response_stream_error(
        &mut state,
        5,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .or_cancel(&token);
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    token.cancel();
    assert!(retry.await.is_err());
    let mut state = ResponsesStreamRetryState::default();
    let mut client_session = session.services.model_client.new_session();
    handle_response_stream_error(
        &mut state,
        5,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::RemoteCompactionV2,
    )
    .await
    .expect_err("new policy is sampling only");
}

#[tokio::test]
async fn capacity_retry_rejects_stale_auth_owner() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let owner = session.services.model_client.auth_owner_generation();
    let mut state =
        ResponsesStreamRetryState::for_sampling(Some(owner.unwrap_or(0).wrapping_add(1)));
    let error = handle_response_stream_error(
        &mut state,
        5,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .await
    .expect_err("new owner must not retry old work");
    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::TurnAborted
    ));
    assert_eq!(state.capacity_retries, 0);
}

#[tokio::test]
async fn capacity_retry_owner_change_wakes_pending_wait() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState::for_sampling(
        session.services.model_client.auth_owner_generation(),
    );
    let manager = session
        .services
        .model_client
        .auth_manager()
        .expect("test auth manager");
    tokio::time::pause();
    let before = Instant::now();
    let retry = handle_response_stream_error(
        &mut state,
        5,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    );
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    // This fixture manager uses a non-existent test home: reload clears only fake cached auth.
    assert!(manager.reload().await);
    let error = retry
        .await
        .expect_err("account ownership change cancels capacity wait");
    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::TurnAborted
    ));
    assert_eq!(Instant::now(), before);
}

#[tokio::test]
async fn capacity_retry_window_includes_previous_attempts_and_other_errors_stay_terminal() {
    let (session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    let mut client_session = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState::for_sampling(
        session.services.model_client.auth_owner_generation(),
    );
    tokio::time::pause();
    state.capacity_started_at = Some(Instant::now() - Duration::from_secs(179));
    handle_response_stream_error(
        &mut state,
        5,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    )
    .await
    .expect_err("new backoff cannot extend the first failure's window");
    for error in [
        CodexErr::QuotaExceeded,
        CodexErr::UsageNotIncluded,
        CodexErr::Fatal("permission denied".into()),
    ] {
        let error = handle_response_stream_error(
            &mut state,
            5,
            error,
            &mut client_session,
            &session,
            &step_context,
            ResponsesStreamRequest::Sampling,
        )
        .await
        .expect_err("capacity policy must not broaden unrelated errors");
        assert!(!matches!(
            error.details(),
            codex_protocol::error::CodexErrorDetails::ServerOverloaded
        ));
    }
    assert_eq!(state.capacity_retries, 0);
}

#[tokio::test]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "delay native event delivery while virtual time advances"
)]
async fn capacity_retry_window_is_rechecked_after_notification_wait() {
    let (mut session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    session.realtime_history = Some(Mutex::new(RealtimeHistoryState::default()));
    let mut client_session = session.services.model_client.new_session();
    let mut state = ResponsesStreamRetryState::for_sampling(
        session.services.model_client.auth_owner_generation(),
    );
    tokio::time::pause();
    let history_guard = session.realtime_history.as_ref().unwrap().lock().await;
    let retry = handle_response_stream_error(
        &mut state,
        5,
        CodexErr::ServerOverloaded,
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    );
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(181)).await;
    drop(history_guard);
    let error = retry
        .await
        .expect_err("notification delivery cannot extend the retry window");
    assert!(matches!(
        error.details(),
        codex_protocol::error::CodexErrorDetails::ServerOverloaded
    ));
    let advice = session
        .services
        .thread_extension_data
        .get::<super::ExhaustedResponseRetry>()
        .expect("outer retry handoff");
    assert_eq!(advice.turn_id, step_context.turn.sub_id);
}

#[tokio::test]
async fn sampling_retry_logs_stream_error_context() {
    let (_session, turn_context) = make_session_and_context().await;
    let buffer: &'static std::sync::Mutex<Vec<u8>> =
        Box::leak(Box::new(std::sync::Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(MockWriter::new(buffer))
        .finish();
    let _subscriber_guard = tracing::subscriber::set_default(subscriber);

    log_retry(
        ResponsesStreamRequest::Sampling,
        &turn_context,
        &CodexErr::Stream("websocket closed by server before response.completed".to_string()),
        /*retries*/ 2,
        /*max_retries*/ 5,
        Duration::from_secs(1),
    );

    let logs = String::from_utf8(
        buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
    )
    .expect("retry log should be valid utf-8");
    assert!(logs.contains("stream disconnected - retrying sampling request"));
    assert!(logs.contains(&format!("turn_id={}", turn_context.sub_id)));
    assert!(logs.contains("retries=2"));
    assert!(logs.contains("max_retries=5"));
    assert!(logs.contains(
        "sampling_error=stream disconnected before completion: websocket closed by server before response.completed"
    ));
}

/// Time spent reporting a retry must count toward the server's original deadline.
#[tokio::test]
#[expect(
    clippy::await_holding_invalid_type,
    reason = "test holds the event-delivery lock to delay notification while virtual time advances"
)]
async fn stream_retry_preserves_deadline_across_delayed_notification() {
    let (mut session, turn_context) = make_session_and_context().await;
    let step_context = StepContext::for_test(Arc::new(turn_context));
    session.realtime_history = Some(Mutex::new(RealtimeHistoryState::default()));
    let mut client_session = session.services.model_client.new_session();
    // The second retry emits a notification even when release builds hide the first.
    let mut retry_state = ResponsesStreamRetryState {
        retries: 1,
        ..Default::default()
    };

    tokio::time::pause();
    let advice = RetryAfter::from_delay(Duration::from_secs(10)).expect("retry deadline");
    tokio::time::advance(Duration::from_secs(4)).await;

    // Hold the event-delivery lock so reporting the error consumes part of the deadline.
    let history_guard = session.realtime_history.as_ref().unwrap().lock().await;
    let retry = handle_response_stream_error(
        &mut retry_state,
        /*max_retries*/ 2,
        CodexErr::InternalServerError.with_retry_after(advice),
        &mut client_session,
        &session,
        &step_context,
        ResponsesStreamRequest::Sampling,
    );
    tokio::pin!(retry);
    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(4)).await;
    drop(history_guard);

    assert!(futures::poll!(&mut retry).is_pending());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(futures::poll!(&mut retry).is_pending());
    retry.await.expect("retry should be allowed");
    // Tokio rounds timer deadlines up to the next millisecond.
    let resumed_at = Instant::now();
    assert!(
        (advice.deadline()..=advice.deadline() + Duration::from_millis(1)).contains(&resumed_at),
        "retry resumed at {resumed_at:?}, expected {advice:?}"
    );
}
