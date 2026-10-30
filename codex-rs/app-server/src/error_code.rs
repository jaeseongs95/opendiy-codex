// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
use codex_app_server_protocol::JSONRPCErrorError;

pub(crate) const INVALID_REQUEST_ERROR_CODE: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND_ERROR_CODE: i64 = -32601;
pub const INVALID_PARAMS_ERROR_CODE: i64 = -32602;
pub(crate) const INTERNAL_ERROR_CODE: i64 = -32603;
pub(crate) const OVERLOADED_ERROR_CODE: i64 = -32001;
pub const INPUT_TOO_LARGE_ERROR_CODE: &str = "input_too_large";

pub(crate) fn server_draining_error() -> JSONRPCErrorError {
    let mut error = invalid_request("Server is draining; retry after reconnecting");
    error.data = Some(serde_json::json!({ "reason": "serverShuttingDown" }));
    error
}

pub(crate) fn invalid_request(message: impl Into<String>) -> JSONRPCErrorError {
    error(INVALID_REQUEST_ERROR_CODE, message)
}

/// Only notLoaded/listenerNotReady allow a bounded retry of subscribePassive.
/// Closing or a changed runtime must never trigger an implicit resume.
pub(crate) fn passive_subscription_unavailable(reason: &'static str) -> JSONRPCErrorError {
    let mut error = invalid_request(format!("passive thread subscription unavailable: {reason}"));
    error.data = Some(serde_json::json!({
        "kind": "threadSubscriptionUnavailable",
        "reason": reason,
    }));
    error
}

pub(crate) fn passive_runtime_unavailable(
    reason: codex_core::LoadedThreadUnavailable,
) -> JSONRPCErrorError {
    use codex_core::LoadedThreadUnavailable;
    passive_subscription_unavailable(match reason {
        LoadedThreadUnavailable::NotLoaded => "notLoaded",
        LoadedThreadUnavailable::Replaced => "runtimeChanged",
        LoadedThreadUnavailable::Closing => "closing",
        LoadedThreadUnavailable::Unsupported => "unsupportedRuntime",
    })
}

pub(crate) fn method_not_found(message: impl Into<String>) -> JSONRPCErrorError {
    error(METHOD_NOT_FOUND_ERROR_CODE, message)
}

pub(crate) fn invalid_params(message: impl Into<String>) -> JSONRPCErrorError {
    error(INVALID_PARAMS_ERROR_CODE, message)
}

pub(crate) fn internal_error(message: impl Into<String>) -> JSONRPCErrorError {
    error(INTERNAL_ERROR_CODE, message)
}

fn error(code: i64, message: impl Into<String>) -> JSONRPCErrorError {
    JSONRPCErrorError {
        code,
        message: message.into(),
        data: None,
    }
}
