// Modified for OpenDIY Codex R30 (2026-10-30); see MODIFICATIONS.md.
//! Bounded structural observations only; never log the response or complete usage object.
use serde_json::Value;

fn container_state(value: Option<&Value>) -> &'static str {
    match value {
        None => "absent",
        Some(Value::Null) => "null",
        Some(Value::Object(_)) => "object",
        Some(_) => "invalid",
    }
}

fn count(value: Option<&Value>) -> (&'static str, Option<u64>) {
    match value {
        None => ("absent", None),
        Some(Value::Null) => ("null", None),
        Some(value) => match value.as_u64() {
            Some(0) => ("zero", Some(0)),
            Some(value) => ("positive", Some(value)),
            None => ("invalid", None),
        },
    }
}

fn label(value: Option<&Value>, max_bytes: usize) -> (&'static str, Option<&str>) {
    match value {
        None => ("absent", None),
        Some(Value::Null) => ("null", None),
        Some(Value::String(value))
            if !value.is_empty()
                && value.len() <= max_bytes
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':')
                }) =>
        {
            ("present", Some(value))
        }
        Some(_) => ("invalid", None),
    }
}

pub(super) fn observe(response: &Value) {
    if !tracing::enabled!(target: "codex_api::prompt_cache", tracing::Level::DEBUG) {
        return;
    }
    let usage = response.get("usage");
    let details = usage.and_then(|usage| usage.get("input_tokens_details"));
    let diagnostics = response.get("prompt_cache_diagnostics");
    let (response_id_state, response_id) = label(response.get("id"), 128);
    let (input_state, input_tokens) = count(usage.and_then(|usage| usage.get("input_tokens")));
    let (cached_state, cached_tokens) =
        count(details.and_then(|details| details.get("cached_tokens")));
    let (write_state, cache_write_tokens) =
        count(details.and_then(|details| details.get("cache_write_tokens")));
    let (model_state, returned_model) = label(response.get("model"), 128);
    let (tier_state, returned_service_tier) = label(response.get("service_tier"), 64);
    let (diagnostics_type_state, diagnostics_type) = label(
        diagnostics.and_then(|diagnostics| diagnostics.get("type")),
        64,
    );
    let (diagnostics_reason_state, diagnostics_reason) = label(
        diagnostics.and_then(|diagnostics| diagnostics.get("reason")),
        64,
    );
    tracing::debug!(
        target: "codex_api::prompt_cache",
        response_id_state,
        response_id = response_id.unwrap_or("unknown"),
        usage_state = container_state(usage),
        input_tokens_details_state = container_state(details),
        input_state,
        input_tokens,
        cached_state,
        cached_tokens,
        write_state,
        cache_write_tokens,
        model_state,
        returned_model = returned_model.unwrap_or("unknown"),
        tier_state,
        returned_service_tier = returned_service_tier.unwrap_or("unknown"),
        diagnostics_state = container_state(diagnostics),
        diagnostics_type_state,
        diagnostics_type = diagnostics_type.unwrap_or("unknown"),
        diagnostics_reason_state,
        diagnostics_reason = diagnostics_reason.unwrap_or("unknown"),
        "prompt_cache_observation"
    );
}

#[cfg(test)]
mod tests {
    use super::super::ResponsesStreamEvent;
    use super::super::process_responses_event;
    use crate::common::ResponseEvent;
    use serde_json::Value;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use tracing::Subscriber;
    use tracing::field::Field;
    use tracing::field::Visit;
    use tracing::span::Attributes;
    use tracing::span::Id;
    use tracing::span::Record;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

    #[derive(Default)]
    struct Fields(BTreeMap<String, String>);

    impl Visit for Fields {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0
                .insert(field.name().to_string(), format!("{value:?}"));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.insert(field.name().to_string(), value.to_string());
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            self.0.insert(field.name().to_string(), value.to_string());
        }
    }

    impl Subscriber for Capture {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}
        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            if event.metadata().target() == "codex_api::prompt_cache" {
                let mut fields = Fields::default();
                event.record(&mut fields);
                self.0.lock().unwrap().push(fields.0);
            }
        }

        fn enter(&self, _span: &Id) {}
        fn exit(&self, _span: &Id) {}
    }

    fn parse(response: Value) -> (bool, BTreeMap<String, String>) {
        let capture = Capture::default();
        let events = Arc::clone(&capture.0);
        let original = response.clone();
        let event: ResponsesStreamEvent = serde_json::from_value(json!({
            "type": "response.completed", "response": response
        }))
        .unwrap();
        let result = tracing::subscriber::with_default(capture, || process_responses_event(event));
        assert!(!matches!(&result, Ok(None)));
        if let Ok(Some(ResponseEvent::Completed { usage_metadata, .. })) = &result {
            assert_eq!(
                usage_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.metadata.as_ref()),
                original.get("usage").filter(|usage| !usage.is_null()),
            );
        }
        let mut events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        (result.is_ok(), events.remove(0))
    }

    fn completed_response() -> Value {
        json!({"id": "resp_fixture", "usage": {
            "input_tokens": 2048, "output_tokens": 1, "total_tokens": 2049
        }})
    }

    #[test]
    fn cache_observation_parser_boundary_preserves_count_states() {
        for (value, expected, number, parses) in [
            (None, "absent", None, true),
            (Some(Value::Null), "null", None, false),
            (Some(json!(0)), "zero", Some("0"), true),
            (Some(json!(1024)), "positive", Some("1024"), true),
            (Some(json!(-1)), "invalid", None, true),
            (Some(json!(1.5)), "invalid", None, false),
            (Some(json!("1024")), "invalid", None, false),
            (Some(json!(true)), "invalid", None, false),
        ] {
            let mut response = completed_response();
            if let Some(value) = value {
                response["usage"]["input_tokens_details"] = json!({"cached_tokens": value});
            }
            let (parsed, fields) = parse(response);
            assert_eq!(fields["cached_state"], expected);
            assert_eq!(fields.get("cached_tokens").map(String::as_str), number);
            assert_eq!(parsed, parses, "{expected}");
        }
        let mut response = completed_response();
        response["usage"]["input_tokens_details"] =
            json!({"cached_tokens": 0, "cache_write_tokens": 4096});
        let (_, fields) = parse(response);
        assert_eq!(fields["input_state"], "positive");
        assert_eq!(fields["input_tokens"], "2048");
        assert_eq!(fields["write_state"], "positive");
        assert_eq!(fields["cache_write_tokens"], "4096");

        for (value, expected, number) in [
            (None, "absent", None),
            (Some(Value::Null), "null", None),
            (Some(json!(0)), "zero", Some("0")),
            (Some(json!(512)), "positive", Some("512")),
            (Some(json!(-1)), "invalid", None),
            (Some(json!(0.0)), "invalid", None),
            (Some(json!("512")), "invalid", None),
        ] {
            for field in ["input_tokens", "cache_write_tokens"] {
                let mut response = completed_response();
                response["usage"]["input_tokens_details"] = json!({"cached_tokens": 0});
                let parent = if field == "input_tokens" {
                    &mut response["usage"]
                } else {
                    &mut response["usage"]["input_tokens_details"]
                };
                parent.as_object_mut().unwrap().remove(field);
                if let Some(value) = value.as_ref() {
                    parent[field] = value.clone();
                }
                let (_, fields) = parse(response);
                let state_field = if field == "input_tokens" {
                    "input_state"
                } else {
                    "write_state"
                };
                assert_eq!(fields[state_field], expected, "{field}");
                assert_eq!(fields.get(field).map(String::as_str), number, "{field}");
            }
        }
    }

    #[test]
    fn cache_observation_parser_boundary_records_returned_labels_only() {
        let mut response = completed_response();
        response["model"] = json!("model-fixture-2");
        response["service_tier"] = json!("priority");
        response["prompt_cache_diagnostics"] =
            json!({"type": "cache_miss", "reason": "service_tier_changed"});
        response["output"] = json!([{"text": "DO_NOT_LOG_PRIVATE_BODY"}]);
        response["usage"]["private_field"] = json!("DO_NOT_LOG_PRIVATE_USAGE");
        let (parsed, fields) = parse(response);
        assert!(parsed);
        assert_eq!(fields["returned_model"], "model-fixture-2");
        assert_eq!(fields["returned_service_tier"], "priority");
        assert_eq!(fields["diagnostics_type"], "cache_miss");
        assert_eq!(fields["diagnostics_reason"], "service_tier_changed");
        assert!(!format!("{fields:?}").contains("DO_NOT_LOG"));

        let (_, fields) = parse(completed_response());
        assert_eq!(fields["model_state"], "absent");
        assert_eq!(fields["tier_state"], "absent");
        assert_eq!(fields["diagnostics_state"], "absent");
        assert_eq!(fields["returned_model"], "unknown");
    }

    #[test]
    fn cache_observation_parser_boundary_rejects_unbounded_or_malformed_labels() {
        let mut response = completed_response();
        response["id"] = json!("x".repeat(128));
        response["model"] = json!("m".repeat(128));
        response["service_tier"] = json!("t".repeat(64));
        response["prompt_cache_diagnostics"] =
            json!({"type": "h".repeat(64), "reason": "r".repeat(64)});
        let (parsed, fields) = parse(response);
        assert!(parsed);
        assert_eq!(fields["response_id"].len(), 128);
        assert_eq!(fields["returned_model"].len(), 128);
        assert_eq!(fields["returned_service_tier"].len(), 64);
        assert_eq!(fields["diagnostics_type"].len(), 64);
        assert_eq!(fields["diagnostics_reason"].len(), 64);

        let mut response = completed_response();
        response["id"] = json!("x".repeat(129));
        response["model"] = json!("DO_NOT_LOG\nPRIVATE");
        response["service_tier"] = Value::Null;
        response["prompt_cache_diagnostics"] = json!({"type": 1, "reason": "x".repeat(65)});
        let (parsed, fields) = parse(response);
        assert!(parsed);
        for name in [
            "response_id_state",
            "model_state",
            "diagnostics_type_state",
            "diagnostics_reason_state",
        ] {
            assert_eq!(fields[name], "invalid");
        }
        assert_eq!(fields["tier_state"], "null");
        assert_eq!(fields["response_id"], "unknown");
        assert!(!format!("{fields:?}").contains("DO_NOT_LOG"));
    }

    #[test]
    fn cache_observation_parser_boundary_distinguishes_container_states() {
        for (usage, state) in [
            (None, "absent"),
            (Some(Value::Null), "null"),
            (Some(json!(true)), "invalid"),
        ] {
            let mut response = json!({"id": "resp_fixture"});
            if let Some(usage) = usage {
                response["usage"] = usage;
            }
            let (_, fields) = parse(response);
            assert_eq!(fields["usage_state"], state);
            assert_eq!(fields["cached_state"], "absent");
        }
        for (value, state) in [(Value::Null, "null"), (json!(true), "invalid")] {
            let mut response = completed_response();
            response["usage"]["input_tokens_details"] = value.clone();
            response["prompt_cache_diagnostics"] = value;
            let (_, fields) = parse(response);
            assert_eq!(fields["input_tokens_details_state"], state);
            assert_eq!(fields["diagnostics_state"], state);
            assert_eq!(fields["diagnostics_type"], "unknown");
        }
    }

    #[test]
    fn cache_observation_ignores_noncompleted_events() {
        for kind in ["response.created", "response.incomplete"] {
            let capture = Capture::default();
            let events = Arc::clone(&capture.0);
            let mut response = completed_response();
            response["incomplete_details"] = json!({"reason": "interrupted"});
            let event =
                serde_json::from_value(json!({"type": kind, "response": response})).unwrap();
            let result =
                tracing::subscriber::with_default(capture, || process_responses_event(event));
            assert!(matches!(
                result,
                Ok(Some(ResponseEvent::Created { .. }))
                    | Ok(Some(ResponseEvent::Completed {
                        end_turn: Some(false),
                        ..
                    }))
            ));
            assert!(events.lock().unwrap().is_empty());
        }
    }
}
