// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Fixed-size transport observations, scoped by the log sink's existing process UUID.
//! Input hashes describe the serialized wire input, not the omitted WS logical prefix.
use crate::common::ResponseCreateWsRequest;
use crate::common::ResponsesApiRequest;
use crate::common::ResponsesApiTools;
use serde::Serialize;
use serde_json::Value;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::hash::Hasher;
use std::io;
use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

static HASH_STATE: OnceLock<RandomState> = OnceLock::new();
static NEXT_ORDINAL: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
std::thread_local! {
    static OBSERVATION_WORK: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fingerprint {
    hash: Option<u64>,
    bytes: u64,
}

// Hasher does not promise write-chunk invariance. Normalize chunks with bounded storage.
struct HashWriter<H> {
    hasher: H,
    pending: [u8; 64],
    len: usize,
    bytes: u64,
}

impl<H: Hasher> HashWriter<H> {
    fn new(hasher: H) -> Self {
        Self {
            hasher,
            pending: [0; 64],
            len: 0,
            bytes: 0,
        }
    }

    fn finish(mut self) -> Fingerprint {
        if self.len != 0 {
            self.hasher.write(&self.pending[..self.len]);
        }
        Fingerprint {
            hash: Some(self.hasher.finish()),
            bytes: self.bytes,
        }
    }
}

impl<H: Hasher> Write for HashWriter<H> {
    fn write(&mut self, mut bytes: &[u8]) -> io::Result<usize> {
        let written = bytes.len();
        self.bytes = self.bytes.saturating_add(written as u64);
        while !bytes.is_empty() {
            let copied = bytes.len().min(self.pending.len() - self.len);
            self.pending[self.len..self.len + copied].copy_from_slice(&bytes[..copied]);
            self.len += copied;
            bytes = &bytes[copied..];
            if self.len == self.pending.len() {
                self.hasher.write(&self.pending);
                self.len = 0;
            }
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn fingerprint(value: &(impl Serialize + ?Sized)) -> Fingerprint {
    #[cfg(test)]
    OBSERVATION_WORK.with(|work| {
        let (hashes, ordinals) = work.get();
        work.set((hashes + 1, ordinals));
    });
    let mut writer = HashWriter::new(HASH_STATE.get_or_init(RandomState::new).build_hasher());
    match serde_json::to_writer(&mut writer, value) {
        Ok(()) => writer.finish(),
        Err(_) => Fingerprint {
            hash: None,
            bytes: writer.bytes,
        },
    }
}

// Layer filters compose with OR: feedback may enable this target independently.
// Check the process opt-in before any serialization, hash work, or ordinal allocation.
fn wire_diagnostics_opted_in() -> bool {
    matches!(
        std::env::var("APP_CACHE_WIRE_DIAGNOSTICS").as_deref(),
        Ok("1")
    )
}

pub(crate) struct WireObservation {
    ordinal: u64,
    terminal: bool,
    dispatch: tracing::Dispatch,
}

impl WireObservation {
    pub(crate) fn http(request: &ResponsesApiRequest) -> Option<Self> {
        if !wire_diagnostics_opted_in() {
            return None;
        }
        Self::http_opted_in(request)
    }

    fn http_opted_in(request: &ResponsesApiRequest) -> Option<Self> {
        if !tracing::enabled!(target: "codex_api::prompt_cache_wire", tracing::Level::DEBUG) {
            return None;
        }
        let observation = Self::new();
        observation.request(
            "http",
            "full",
            false,
            false,
            &request.model,
            request.service_tier.as_deref(),
            request.prompt_cache_key.as_deref(),
            request.tools.as_ref().map(ResponsesApiTools::as_raw_value),
            &request.input,
            request.client_metadata.as_ref(),
            &(
                &request.tool_choice,
                request.parallel_tool_calls,
                &request.reasoning,
                request.store,
                request.stream,
                &request.stream_options,
                &request.include,
                &request.text,
                &request.access_programs,
                None::<bool>,
            ),
            None,
        );
        Some(observation)
    }

    pub(crate) fn websocket(
        request: &ResponseCreateWsRequest<'_>,
        connection_reused: bool,
    ) -> Option<Self> {
        if !wire_diagnostics_opted_in() {
            return None;
        }
        Self::websocket_opted_in(request, connection_reused)
    }

    fn websocket_opted_in(
        request: &ResponseCreateWsRequest<'_>,
        connection_reused: bool,
    ) -> Option<Self> {
        if !tracing::enabled!(target: "codex_api::prompt_cache_wire", tracing::Level::DEBUG) {
            return None;
        }
        let observation = Self::new();
        observation.request(
            "websocket",
            if request.previous_response_id.is_some() {
                "delta"
            } else {
                "full"
            },
            request.generate == Some(false),
            connection_reused,
            request.model,
            request.service_tier,
            request.prompt_cache_key,
            request.tools,
            request.input,
            request.client_metadata.as_ref(),
            &(
                request.tool_choice,
                request.parallel_tool_calls,
                request.reasoning,
                request.store,
                request.stream,
                request.stream_options,
                request.include,
                request.text,
                request.access_programs,
                request.generate,
            ),
            request.previous_response_id.as_deref(),
        );
        Some(observation)
    }

    fn new() -> Self {
        #[cfg(test)]
        OBSERVATION_WORK.with(|work| {
            let (hashes, ordinals) = work.get();
            work.set((hashes, ordinals + 1));
        });
        Self {
            ordinal: NEXT_ORDINAL.fetch_add(1, Ordering::Relaxed),
            terminal: false,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn request(
        &self,
        transport: &'static str,
        mode: &'static str,
        warmup: bool,
        connection_reused: bool,
        model: &str,
        tier: Option<&str>,
        key: Option<&str>,
        tools: Option<&serde_json::value::RawValue>,
        input: &[codex_protocol::models::ResponseItem],
        metadata: Option<&std::collections::HashMap<String, String>>,
        controls: &impl Serialize,
        previous_response_id: Option<&str>,
    ) {
        let model = fingerprint(model);
        let tier_hash = tier.map(fingerprint);
        let key_hash = key.map(fingerprint);
        let tools_hash = tools.map(fingerprint);
        let input_hash = fingerprint(input);
        let controls_hash = fingerprint(controls);
        let metadata_hash = metadata.map(fingerprint);
        let previous_response_hash = previous_response_id.map(fingerprint);
        tracing::debug!(
            target: "codex_api::prompt_cache_wire",
            wire_ordinal = self.ordinal,
            transport,
            mode,
            warmup,
            connection_reused,
            logical_prefix = "unknown",
            model_hash = ?model.hash,
            tier_present = tier.is_some(),
            tier_hash = ?tier_hash.and_then(|hash| hash.hash),
            cache_key_present = key.is_some(),
            cache_key_hash = ?key_hash.and_then(|hash| hash.hash),
            tools_present = tools.is_some(),
            tools_hash = ?tools_hash.and_then(|hash| hash.hash),
            wire_input_hash = ?input_hash.hash,
            input_items = input.len(),
            input_bytes = input_hash.bytes,
            controls_hash = ?controls_hash.hash,
            metadata_present = metadata.is_some(),
            metadata_hash = ?metadata_hash.and_then(|hash| hash.hash),
            previous_response_hash = ?previous_response_hash.and_then(|hash| hash.hash),
            "prompt_cache_wire_request"
        );
    }

    pub(crate) fn stage(&mut self, stage: &'static str) {
        self.event(stage, None, None);
        self.terminal |= matches!(stage, "transport_error" | "send_error");
    }

    pub(crate) fn upstream(&mut self, id: Option<&str>) {
        self.event("stream_opened", None, id);
    }

    fn event(&self, stage: &'static str, response: Option<&Value>, upstream_id: Option<&str>) {
        // Same bounded response-ID rule as the raw-presence event, to join without prompt data.
        let response_id = response
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 128
                    && id.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':')
                    })
            });
        tracing::dispatcher::with_default(&self.dispatch, || {
            tracing::debug!(
                target: "codex_api::prompt_cache_wire",
                wire_ordinal = self.ordinal,
                stage,
                response_id = response_id.unwrap_or("unknown"),
                upstream_request_hash = ?upstream_id.map(fingerprint).and_then(|hash| hash.hash),
                returned_model_hash = ?response.and_then(|value| value.get("model")).and_then(|value| value.as_str()).map(fingerprint).and_then(|hash| hash.hash),
                returned_tier_hash = ?response.and_then(|value| value.get("service_tier")).and_then(|value| value.as_str()).map(fingerprint).and_then(|hash| hash.hash),
                "prompt_cache_wire_response"
            )
        });
    }

    pub(crate) fn response(&mut self, kind: &str, response: Option<&Value>) {
        let stage = match kind {
            "response.created" => "created",
            "response.completed" => "completed",
            "response.incomplete" => "incomplete",
            "response.failed" => "failed",
            _ => return,
        };
        self.event(stage, response, None);
        self.terminal |= stage != "created";
    }
}

impl Drop for WireObservation {
    fn drop(&mut self) {
        if !self.terminal {
            self.event("abandoned_before_terminal", None, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::ResponsesWsRequest;
    use codex_client::EncodedJsonBody;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::hint::black_box;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Instant;
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
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            if event.metadata().target() == "codex_api::prompt_cache_wire" {
                let mut fields = Fields::default();
                event.record(&mut fields);
                self.0.lock().unwrap().push(fields.0);
            }
        }
    }

    fn fixture() -> ResponsesApiRequest {
        ResponsesApiRequest {
            model: "private-model-sentinel".to_string(),
            stream: true,
            service_tier: Some("private-tier-sentinel".to_string()),
            input: vec![
                serde_json::from_value(json!({
                    "type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "private-input-sentinel"}]
                }))
                .unwrap(),
            ],
            tools: Some(
                Arc::<serde_json::value::RawValue>::from(
                    serde_json::value::to_raw_value(&json!([
                        {"type": "function", "name": "private-tool-sentinel"}
                    ]))
                    .unwrap(),
                )
                .into(),
            ),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
            reasoning: None,
            store: false,
            stream_options: None,
            include: Vec::new(),
            prompt_cache_key: Some("private-key-sentinel".to_string()),
            text: None,
            client_metadata: None,
            access_programs: None,
        }
    }

    fn request_fields(request: &ResponsesApiRequest) -> BTreeMap<String, String> {
        let capture = Capture::default();
        let events = Arc::clone(&capture.0);
        tracing::subscriber::with_default(capture, || {
            drop(WireObservation::http_opted_in(request));
        });
        let fields = events.lock().unwrap()[0].clone();
        fields
    }

    #[test]
    fn wire_process_opt_in_controls_work_before_layer_filters() {
        const CHILD: &str = "CODEX_WIRE_OPTIN_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let expected = std::env::var("APP_CACHE_WIRE_DIAGNOSTICS").as_deref() == Ok("1");
            tracing::subscriber::with_default(Capture::default(), || {
                // This always-enabled subscriber reproduces the feedback layer's target admission.
                assert!(
                    tracing::enabled!(target: "codex_api::prompt_cache_wire", tracing::Level::DEBUG)
                );
                let before = OBSERVATION_WORK.with(std::cell::Cell::get);
                let request = fixture();
                assert_eq!(WireObservation::http(&request).is_some(), expected);
                assert_eq!(
                    WireObservation::websocket(&ResponseCreateWsRequest::from(&request), false)
                        .is_some(),
                    expected
                );
                let after = OBSERVATION_WORK.with(std::cell::Cell::get);
                if expected {
                    assert!(after.0 > before.0 && after.1 > before.1);
                } else {
                    assert_eq!(
                        after, before,
                        "opt-out must perform no hash or ordinal work"
                    );
                }
            });
            return;
        }
        for value in [None, Some("0"), Some("true"), Some("1")] {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "cache_wire_observation::tests::wire_process_opt_in_controls_work_before_layer_filters", "--nocapture"]);
            command.env(CHILD, "1");
            if let Some(value) = value {
                command.env("APP_CACHE_WIRE_DIAGNOSTICS", value);
            } else {
                command.env_remove("APP_CACHE_WIRE_DIAGNOSTICS");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "opt-in {value:?}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }

    #[test]
    fn wire_hash_is_independent_of_writer_chunking() {
        let bytes = (0..513)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let state = RandomState::new();
        let mut expected = None;
        for chunk_size in [1, 3, 63, 64, 65, 513] {
            let mut writer = HashWriter::new(state.build_hasher());
            for chunk in bytes.chunks(chunk_size) {
                writer.write_all(chunk).unwrap();
            }
            let actual = writer.finish();
            assert_eq!(actual.bytes, bytes.len() as u64);
            if let Some(expected) = expected {
                assert_eq!(expected, actual);
            } else {
                expected = Some(actual);
            }
        }
        assert_eq!(fingerprint("same"), fingerprint(&"same".to_string()));
    }

    #[test]
    fn wire_components_ignore_metadata_and_separate_actual_changes() {
        let mut request = fixture();
        let baseline = request_fields(&request);
        request.client_metadata = Some(std::collections::HashMap::from([(
            "timestamp".to_string(),
            "private-header-sentinel".to_string(),
        )]));
        let metadata_only = request_fields(&request);
        for field in [
            "model_hash",
            "tier_hash",
            "cache_key_hash",
            "tools_hash",
            "wire_input_hash",
            "controls_hash",
        ] {
            assert_eq!(baseline[field], metadata_only[field]);
        }
        assert_ne!(baseline["metadata_hash"], metadata_only["metadata_hash"]);
        request.model.push('x');
        assert_ne!(
            baseline["model_hash"],
            request_fields(&request)["model_hash"]
        );
        request.service_tier = None;
        assert_ne!(baseline["tier_hash"], request_fields(&request)["tier_hash"]);
        request.prompt_cache_key = None;
        assert_ne!(
            baseline["cache_key_hash"],
            request_fields(&request)["cache_key_hash"]
        );
        request.tools = None;
        assert_ne!(
            baseline["tools_hash"],
            request_fields(&request)["tools_hash"]
        );
        request.input.clear();
        assert_ne!(
            baseline["wire_input_hash"],
            request_fields(&request)["wire_input_hash"]
        );
        let logged = format!("{baseline:?}{metadata_only:?}");
        assert!(!logged.contains("sentinel"));
    }

    #[test]
    fn wire_observation_preserves_http_and_websocket_serialization() {
        let request = fixture();
        let http_before = EncodedJsonBody::encode(&request).unwrap();
        let capture = Capture::default();
        let events = Arc::clone(&capture.0);
        tracing::subscriber::with_default(capture, || {
            drop(WireObservation::http_opted_in(&request));
            let ws = ResponsesWsRequest::ResponseCreate(ResponseCreateWsRequest {
                previous_response_id: Some("resp_previous".to_string()),
                ..ResponseCreateWsRequest::from(&request)
            });
            let before = serde_json::to_string(&ws).unwrap();
            let ResponsesWsRequest::ResponseCreate(ws_request) = &ws;
            drop(WireObservation::websocket_opted_in(ws_request, true));
            let after = serde_json::to_string(&ws).unwrap();
            assert_eq!(before, after);
        });
        let events = events.lock().unwrap();
        for field in [
            "model_hash",
            "tier_hash",
            "cache_key_hash",
            "tools_hash",
            "wire_input_hash",
            "controls_hash",
        ] {
            assert_eq!(events[0][field], events[2][field]);
        }
        assert_eq!(events[0]["mode"], "full");
        assert_eq!(events[2]["mode"], "delta");
        assert_eq!(events[2]["logical_prefix"], "unknown");
        assert_eq!(http_before, EncodedJsonBody::encode(&request).unwrap());
        tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
            let before = OBSERVATION_WORK.with(std::cell::Cell::get);
            assert!(WireObservation::http_opted_in(&request).is_none());
            assert!(
                WireObservation::websocket_opted_in(
                    &ResponseCreateWsRequest::from(&request),
                    false
                )
                .is_none()
            );
            assert_eq!(OBSERVATION_WORK.with(std::cell::Cell::get), before);
        });
    }

    #[test]
    fn wire_ordinals_follow_reversed_responses_and_abandoned_requests() {
        let capture = Capture::default();
        let events = Arc::clone(&capture.0);
        tracing::subscriber::with_default(capture, || {
            let mut first = WireObservation::new();
            let first_id = first.ordinal.to_string();
            let mut second = WireObservation::new();
            let second_id = second.ordinal.to_string();
            second.response("response.completed", Some(&json!({"id":"resp_second"})));
            first.response("response.completed", Some(&json!({"id":"resp_first"})));
            drop(first);
            drop(second);
            let cancelled = WireObservation::new();
            let cancelled_id = cancelled.ordinal.to_string();
            drop(cancelled);
            let mut failed = WireObservation::new();
            let failed_id = failed.ordinal.to_string();
            failed.stage("send_error");
            drop(failed);
            let fields = events.lock().unwrap();
            assert_eq!(fields.len(), 4);
            for (index, ordinal, response) in
                [(0, second_id, "resp_second"), (1, first_id, "resp_first")]
            {
                assert_eq!(fields[index]["wire_ordinal"], ordinal);
                assert_eq!(fields[index]["response_id"], response);
                assert_eq!(fields[index]["stage"], "completed");
            }
            assert_eq!(fields[2]["wire_ordinal"], cancelled_id);
            assert_eq!(fields[2]["stage"], "abandoned_before_terminal");
            assert_eq!(fields[3]["wire_ordinal"], failed_id);
            assert_eq!(fields[3]["stage"], "send_error");
        });
    }

    #[test]
    fn wire_returned_labels_use_the_request_fingerprint_format() {
        let request = fixture();
        let fields = request_fields(&request);
        let capture = Capture::default();
        let events = Arc::clone(&capture.0);
        tracing::subscriber::with_default(capture, || {
            let mut observation = WireObservation::new();
            observation.response(
                "response.completed",
                Some(&json!({
                    "id": "resp_same", "model": request.model, "service_tier": request.service_tier
                })),
            );
            let mut other = WireObservation::new();
            other.response(
                "response.completed",
                Some(&json!({
                    "id": "resp_different", "model": "different", "service_tier": "different"
                })),
            );
        });
        let events = events.lock().unwrap();
        assert_eq!(fields["model_hash"], events[0]["returned_model_hash"]);
        assert_eq!(fields["tier_hash"], events[0]["returned_tier_hash"]);
        assert_ne!(fields["model_hash"], events[1]["returned_model_hash"]);
        assert_ne!(fields["tier_hash"], events[1]["returned_tier_hash"]);
    }

    #[tokio::test]
    async fn wire_observation_survives_the_http_stream_task() {
        use crate::common::ResponseEvent;
        use crate::sse::responses::spawn_response_stream_observed;
        use futures::StreamExt;

        let capture = Capture::default();
        let events = Arc::clone(&capture.0);
        let dispatch = tracing::Dispatch::new(capture);
        let observation = tracing::dispatcher::with_default(&dispatch, || {
            WireObservation::http_opted_in(&fixture())
        });
        let ordinal = observation.as_ref().unwrap().ordinal.to_string();
        let bytes = futures::stream::iter(vec![Ok(bytes::Bytes::from_static(
            b"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_task\",\"usage\":{\"input_tokens\":2048,\"output_tokens\":1,\"total_tokens\":2049}}}\n\n",
        ))]);
        let response = codex_client::StreamResponse {
            status: http::StatusCode::OK,
            headers: http::HeaderMap::new(),
            bytes: Box::pin(bytes),
        };
        let mut stream = spawn_response_stream_observed(
            response,
            std::time::Duration::from_secs(1),
            None,
            None,
            observation,
        );
        // The observation retains its originating dispatcher across the existing SSE task.
        assert!(matches!(
            stream.next().await,
            Some(Ok(ResponseEvent::RateLimits(_)))
        ));
        let event = stream.next().await;
        assert!(
            matches!(event, Some(Ok(ResponseEvent::Completed { response_id, .. })) if response_id == "resp_task")
        );
        let fields = events.lock().unwrap();
        assert!(fields.iter().any(|fields| {
            fields
                .get("stage")
                .is_some_and(|stage| stage == "completed")
                && fields.get("wire_ordinal") == Some(&ordinal)
                && fields
                    .get("response_id")
                    .is_some_and(|id| id == "resp_task")
        }));
    }

    #[test]
    fn wire_local_encode_observation_cost() {
        for input_bytes in [64, 65_536, 1_048_576] {
            let mut request = fixture();
            request.input = vec![
                serde_json::from_value(json!({
                    "type": "message", "role": "user",
                    "content": [{"type": "input_text", "text": "x".repeat(input_bytes)}]
                }))
                .unwrap(),
            ];
            let mut baseline = Vec::new();
            let mut observed = Vec::new();
            for _ in 0..32 {
                let start = Instant::now();
                black_box(EncodedJsonBody::encode(black_box(&request)).unwrap());
                baseline.push(start.elapsed().as_nanos());
                let start = Instant::now();
                black_box(EncodedJsonBody::encode(black_box(&request)).unwrap());
                black_box(fingerprint(black_box(request.input.as_slice())));
                black_box(fingerprint(black_box(
                    request.tools.as_ref().unwrap().as_raw_value(),
                )));
                observed.push(start.elapsed().as_nanos());
            }
            baseline.sort_unstable();
            observed.sort_unstable();
            println!(
                "wire_observation_cost input_bytes={input_bytes} iterations=32 baseline_median_ns={} baseline_max_ns={} encode_plus_input_tools_hash_median_ns={} encode_plus_input_tools_hash_max_ns={}",
                baseline[16], baseline[31], observed[16], observed[31]
            );
        }
    }
}
