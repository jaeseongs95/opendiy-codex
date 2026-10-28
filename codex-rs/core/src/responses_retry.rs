//! Shared retry and transport fallback decisions for Responses requests.
//! Content-filter guidance is recorded for sampling requests before retry decisions.
//! Server advice controls timing without extending configured retry limits.

use std::time::Duration;

use crate::client::ModelClientSession;
use crate::context::ContentFilterGuidance;
use crate::context::ContextualUserFragment;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::turn_context::TurnContext;
use codex_client::RetryOperation;
use codex_features::Feature;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use tokio::time::Instant;
use tracing::warn;

const INITIAL_CONNECTION_RETRY_DELAY: Duration = Duration::from_secs(5);
const MAX_CONNECTION_RETRY_DELAY: Duration = Duration::from_secs(60);
const MAX_CAPACITY_RETRIES: u64 = 5;
const CAPACITY_RETRY_WINDOW: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResponsesStreamRequest {
    Sampling,
    RemoteCompactionV2,
}

pub(crate) struct ResponsesStreamRetryState {
    retries: u64,
    connection_retries: u64,
    connection_retry_delay: Duration,
    capacity_retries: u64,
    capacity_started_at: Option<Instant>,
    capacity_auth_owner: Option<u64>,
}

impl Default for ResponsesStreamRetryState {
    fn default() -> Self {
        Self {
            retries: 0,
            connection_retries: 0,
            connection_retry_delay: INITIAL_CONNECTION_RETRY_DELAY,
            capacity_retries: 0,
            capacity_started_at: None,
            capacity_auth_owner: None,
        }
    }
}

impl ResponsesStreamRetryState {
    pub(crate) fn for_sampling(auth_owner: Option<u64>) -> Self {
        Self {
            capacity_auth_owner: auth_owner,
            ..Self::default()
        }
    }
}

/// Server retry advice retained after stream retries are exhausted. The turn ID
/// prevents a reused Guardian session from applying advice from an earlier review.
pub(crate) struct ExhaustedResponseRetry {
    pub(crate) turn_id: String,
    pub(crate) retry_at: Option<tokio::time::Instant>,
}

pub(crate) fn exhausted_response_retry(
    sess: &Session,
    turn_context: &TurnContext,
    err: CodexErr,
) -> CodexErr {
    sess.services
        .thread_extension_data
        .insert(ExhaustedResponseRetry {
            turn_id: turn_context.sub_id.clone(),
            retry_at: err.retry_after().map(RetryAfter::deadline),
        });
    err
}

/// A capacity retry belongs to the account that admitted the original request.
/// `None` leaves ordinary sampling's existing ownership behavior unchanged.
pub(crate) async fn capacity_auth_owner_changed(sess: &Session, owner: Option<Option<u64>>) {
    let Some(owner) = owner else {
        return std::future::pending().await;
    };
    let Some(manager) = sess.services.model_client.auth_manager() else {
        return std::future::pending().await;
    };
    let mut changes = manager.auth_change_state_receiver();
    loop {
        if sess.services.model_client.auth_owner_generation() != owner {
            return;
        }
        if changes.changed().await.is_err() {
            return;
        }
        // Refresh notifications must yield so stop/new-input futures can still be polled.
        tokio::task::yield_now().await;
    }
}

/// Returns `Ok(())` when the caller should retry the request loop, or the original error when
/// it is terminal or the retry budget is exhausted.
pub(crate) async fn handle_response_stream_error(
    retry_state: &mut ResponsesStreamRetryState,
    max_retries: u64,
    err: CodexErr,
    client_session: &mut ModelClientSession,
    sess: &Session,
    step_context: &StepContext,
    request: ResponsesStreamRequest,
) -> Result<(), CodexErr> {
    let turn_context = &step_context.turn;
    // Capacity is an admission failure, not a broken transport. Keep it in the same
    // sampling loop without resetting its budget by falling back to another transport.
    if matches!(request, ResponsesStreamRequest::Sampling)
        && matches!(err.details(), CodexErrorDetails::ServerOverloaded)
    {
        let now = Instant::now();
        let started_at = *retry_state.capacity_started_at.get_or_insert(now);
        let capacity_limit = max_retries.min(MAX_CAPACITY_RETRIES);
        let retry_count = retry_state.capacity_retries.saturating_add(1);
        if retry_state.capacity_retries >= capacity_limit || retry_state.retries >= max_retries {
            return Err(exhausted_response_retry(sess, turn_context, err));
        }
        let local_delay = codex_client::backoff(INITIAL_CONNECTION_RETRY_DELAY, retry_count)
            .min(MAX_CONNECTION_RETRY_DELAY);
        let retry_at = err
            .retry_after()
            .map(RetryAfter::deadline)
            .unwrap_or(now)
            .max(now + local_delay);
        if retry_at >= started_at + CAPACITY_RETRY_WINDOW {
            return Err(exhausted_response_retry(sess, turn_context, err));
        }
        let mut auth_changes = sess
            .services
            .model_client
            .auth_manager()
            .map(|manager| manager.auth_change_state_receiver());
        if sess.services.model_client.auth_owner_generation() != retry_state.capacity_auth_owner {
            return Err(CodexErr::TurnAborted);
        }
        retry_state.capacity_retries = retry_count;
        retry_state.retries = retry_state.retries.saturating_add(1);
        sess.notify_stream_error(
            turn_context,
            format!("Model at capacity. Retrying... {retry_count}/{capacity_limit}"),
            CodexErr::ServerOverloaded,
        )
        .await;
        codex_client::record_retry!(
            retry_count,
            retry_at.saturating_duration_since(now),
            RetryOperation::Sampling,
        );
        // The caller applies turn cancellation and new-input preemption to this future.
        // Watch ownership, rather than token refresh, so an account switch cannot resume it.
        loop {
            if Instant::now() >= started_at + CAPACITY_RETRY_WINDOW {
                return Err(exhausted_response_retry(sess, turn_context, err));
            }
            if sess.services.model_client.auth_owner_generation() != retry_state.capacity_auth_owner
            {
                return Err(CodexErr::TurnAborted);
            }
            tokio::select! {
                biased;
                changed = async {
                    match auth_changes.as_mut() {
                        Some(changes) => changes.changed().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if changed.is_err() {
                        return Err(CodexErr::TurnAborted);
                    }
                }
                _ = tokio::time::sleep_until(retry_at) => {
                    return if Instant::now() < started_at + CAPACITY_RETRY_WINDOW {
                        Ok(())
                    } else {
                        Err(exhausted_response_retry(sess, turn_context, err))
                    };
                }
            }
        }
    }
    if matches!(request, ResponsesStreamRequest::Sampling)
        && matches!(err.details(), CodexErrorDetails::ContentFilter)
    {
        let model_info = &step_context.settings.model_info;
        let guidance = ContentFilterGuidance {
            text: codex_prompts::ResolvedModelMessages::from_model(model_info)
                .content_filter_guidance()
                .to_string(),
        };
        sess.record_conversation_items(
            turn_context,
            model_info,
            &[ContextualUserFragment::into(guidance)],
        )
        .await;
    }
    let operation = match request {
        ResponsesStreamRequest::Sampling => RetryOperation::Sampling,
        ResponsesStreamRequest::RemoteCompactionV2 => RetryOperation::RemoteCompactionV2,
    };
    let retry_count = retry_state.retries.saturating_add(1);
    let Some(delay) = err.retry_delay(retry_count) else {
        return Err(err);
    };
    let retry_after = err.retry_after();

    if turn_context
        .config
        .features
        .enabled(Feature::UnboundedConnectionRetries)
        && matches!(request, ResponsesStreamRequest::Sampling)
        && matches!(err.details(), CodexErrorDetails::ConnectionFailed(_))
        && !turn_context.session_source.is_internal()
        && !turn_context.provider.info().is_amazon_bedrock()
    {
        let retry_delay = retry_state.connection_retry_delay;
        warn!(
            turn_id = %turn_context.sub_id,
            error = %err,
            ?retry_delay,
            "stream connection failed; waiting to retry"
        );
        sess.notify_stream_error(turn_context, "Reconnecting... waiting for network", err)
            .await;
        retry_state.connection_retries = retry_state.connection_retries.saturating_add(1);
        codex_client::record_retry!(retry_state.connection_retries, retry_delay, operation);
        tokio::time::sleep(retry_delay).await;
        retry_state.connection_retry_delay = retry_delay
            .saturating_mul(2)
            .min(MAX_CONNECTION_RETRY_DELAY);
        return Ok(());
    }

    if retry_state.retries >= max_retries
        && client_session.try_switch_fallback_transport(
            &turn_context.session_telemetry,
            turn_context.model_info(),
        )
    {
        // Changing transport must not bypass the server's retry deadline.
        if let Some(retry_after) = retry_after {
            tokio::time::sleep_until(retry_after.deadline()).await;
        }
        sess.send_event(
            turn_context,
            EventMsg::Warning(WarningEvent {
                message: format!("Falling back from WebSockets to HTTPS transport. {err:#}"),
            }),
        )
        .await;
        retry_state.retries = 0;
        return Ok(());
    }

    if retry_state.retries < max_retries {
        retry_state.retries = retry_count;
        log_retry(request, turn_context, &err, retry_count, max_retries, delay);

        // In release builds, hide the first websocket retry notification to reduce noisy
        // transient reconnect messages. In debug builds, keep full visibility for diagnosis.
        let report_error = retry_count > 1
            || cfg!(debug_assertions)
            || !sess.services.model_client.responses_websocket_enabled();
        if report_error {
            // Surface retry information to any UI/front-end so the user understands what is
            // happening instead of staring at a seemingly frozen screen.
            sess.notify_stream_error(
                turn_context,
                format!("Reconnecting... {retry_count}/{max_retries}"),
                err,
            )
            .await;
        }
        // Use one clock sample so local backoff telemetry retains the selected delay.
        let now = Instant::now();
        let retry_at = retry_after.map(RetryAfter::deadline).unwrap_or(now + delay);
        let delay = retry_at.saturating_duration_since(now);
        codex_client::record_retry!(retry_count, delay, operation);
        tokio::time::sleep_until(retry_at).await;
        return Ok(());
    }

    Err(exhausted_response_retry(sess, turn_context, err))
}

fn log_retry(
    request: ResponsesStreamRequest,
    turn_context: &TurnContext,
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
    delay: Duration,
) {
    match request {
        ResponsesStreamRequest::Sampling => {
            warn!(
                turn_id = %turn_context.sub_id,
                retries,
                max_retries,
                sampling_error = %err,
                "stream disconnected - retrying sampling request ({retries}/{max_retries} in {delay:?})...",
            );
        }
        ResponsesStreamRequest::RemoteCompactionV2 => {
            warn!(
                turn_id = %turn_context.sub_id,
                retries,
                max_retries,
                compact_error = %err,
                "remote compaction v2 stream failed; retrying request after delay"
            );
        }
    }
}

#[cfg(test)]
#[path = "responses_retry_tests.rs"]
mod tests;
