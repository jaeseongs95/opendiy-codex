// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
//! Exercise real local HTTP/WebSocket requests through the production parser and batch writer.
use codex_api::{
    AuthProvider, Provider, ResponseCreateWsRequest, ResponseEvent, ResponsesApiRequest,
    ResponsesClient, ResponsesOptions, ResponsesWebsocketClient, ResponsesWsRequest, RetryConfig,
};
use codex_client::ReqwestTransport;
use codex_http_client::{HttpClientBuilder, HttpClientFactory, OutboundProxyPolicy};
use futures::{SinkExt, StreamExt};
use http::HeaderMap;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tracing::instrument::WithSubscriber;
use tracing_subscriber::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct NoAuth;
impl AuthProvider for NoAuth {
    fn add_auth_headers(&self, _: &mut HeaderMap) -> Result<(), codex_api::AuthError> {
        Ok(())
    }
}

fn provider(base_url: String) -> Provider {
    Provider {
        name: "local cache fixture".into(),
        base_url,
        query_params: None,
        headers: HeaderMap::new(),
        retry: RetryConfig {
            max_attempts: 1,
            base_delay: Duration::from_millis(1),
            retry_429: false,
            retry_5xx: false,
            retry_transport: false,
        },
        stream_idle_timeout: Duration::from_secs(5),
    }
}

fn request() -> anyhow::Result<ResponsesApiRequest> {
    Ok(ResponsesApiRequest {
        model: "model-fixture".into(),
        stream: true,
        service_tier: Some("priority".into()),
        input: vec![serde_json::from_value(
            json!({"type":"message", "role":"user",
            "content":[{"type":"input_text","text":"PRIVATE_PROMPT"}]}),
        )?],
        tools: None,
        tool_choice: "auto".into(),
        parallel_tool_calls: false,
        reasoning: None,
        store: false,
        stream_options: None,
        include: vec![],
        prompt_cache_key: Some("PRIVATE_CACHE_KEY".into()),
        text: None,
        client_metadata: Some([("fixture".into(), "PRIVATE_METADATA".into())].into()),
        access_programs: None,
    })
}

fn events(id: &str, cached: Option<u64>) -> [Value; 2] {
    let mut details = json!({"cache_write_tokens":0});
    if let Some(cached) = cached {
        details["cached_tokens"] = cached.into();
    }
    [
        json!({"type":"response.created","response":{"id":id}}),
        json!({"type":"response.completed","response":{"id":id,
       "model":"model-fixture", "service_tier":"priority",
       "usage":{"input_tokens":2048,"input_tokens_details":details,
         "output_tokens":1,"total_tokens":2049},
       "prompt_cache_diagnostics":{"type":"cache_miss","reason":"fixture"}}}),
    ]
}

#[test]
fn http_and_websocket_producers_reach_immutable_batches() {
    for wire in [false, true] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "boundary_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("APP_CACHE_WIRE_DIAGNOSTICS", if wire { "1" } else { "0" });
        for name in [
            "CODEX_HOME",
            "OPENAI_API_KEY",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
        ] {
            command.env_remove(name);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        assert!(command.status().unwrap().success(), "wire={wire}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "isolated process for immutable diagnostic startup"]
async fn boundary_child() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    std::fs::write(
        home.path().join("cli-logging.toml"),
        "enabled = true\nprocess = false\nrpc = false\nevents = false\n",
    )?;
    std::fs::write(
        home.path().join("cli-cache-diagnostics.toml"),
        "response = true\nwire = true\n",
    )?;
    let wire_enabled = std::env::var("APP_CACHE_WIRE_DIAGNOSTICS")? == "1";
    let (layer, guard) = codex_diagnostics::batch_log::start(home.path())?;
    let layer = layer.expect("batch enabled");
    let filter = layer.metadata_filter();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(layer.with_filter(tracing_subscriber::filter::filter_fn(filter))),
    );
    let received = async {
        let server = MockServer::start().await;
        let body = events("resp_http", Some(1024))
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>();
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = ResponsesClient::new(
            ReqwestTransport::from_http_client(HttpClientBuilder::new().build_direct()?),
            provider(server.uri()),
            Arc::new(NoAuth),
        );
        let mut stream = client
            .stream_request(request()?, ResponsesOptions::default())
            .await?;
        let mut completed = 0;
        while let Some(event) = stream.next().await {
            if matches!(event?, ResponseEvent::Completed { .. }) {
                completed += 1;
            }
        }
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let actual: Value = serde_json::from_slice(&requests[0].body)?;
        assert_eq!(actual["prompt_cache_key"], "PRIVATE_CACHE_KEY");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(socket).await?;
            for (id, cached, previous) in [
                ("resp_ws_full", Some(0), None),
                ("resp_ws_delta", None, Some("resp_ws_full")),
            ] {
                let text = socket.next().await.unwrap()?.into_text()?;
                let value: Value = serde_json::from_str(&text)?;
                assert_eq!(value["type"], "response.create");
                assert_eq!(
                    value.get("previous_response_id").and_then(Value::as_str),
                    previous
                );
                for event in events(id, cached) {
                    socket
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            event.to_string().into(),
                        ))
                        .await?;
                }
            }
            Ok::<_, anyhow::Error>(())
        });
        let client =
            ResponsesWebsocketClient::new(provider(format!("http://{address}")), Arc::new(NoAuth));
        let connection = client
            .connect(
                &HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
                HeaderMap::new(),
                HeaderMap::new(),
                None,
                None,
            )
            .await?;
        let request = request()?;
        for previous in [None, Some("resp_ws_full".to_string())] {
            let mut ws_request = ResponseCreateWsRequest::from(&request);
            ws_request.previous_response_id = previous.clone();
            let mut stream = connection
                .stream_request(
                    ResponsesWsRequest::ResponseCreate(ws_request),
                    previous.is_some(),
                    None,
                )
                .await?;
            while let Some(event) = stream.next().await {
                if matches!(event?, ResponseEvent::Completed { .. }) {
                    completed += 1;
                }
            }
        }
        server.await??;
        Ok::<_, anyhow::Error>(completed)
    }
    .with_subscriber(dispatch);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(20), received).await??,
        3
    );
    drop(guard);
    let spool = home.path().join("log/cli-batches");
    let mut batches = Vec::new();
    for day in std::fs::read_dir(&spool)? {
        for file in std::fs::read_dir(day?.path())? {
            let path = file?.path();
            assert_eq!(path.extension().unwrap(), "jsonl");
            batches.push((path.clone(), std::fs::read(path)?));
        }
    }
    assert!(!batches.is_empty());
    let rows = batches
        .iter()
        .flat_map(|(_, bytes)| bytes.split(|b| *b == b'\n'))
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let startup = rows
        .iter()
        .find(|row| row["kind"] == "diagnostic_start")
        .unwrap();
    let instance = startup["instance_id"].as_str().unwrap();
    assert_eq!(startup["run"]["cache"]["wire_enabled"], wire_enabled);
    let observations: Vec<_> = rows
        .iter()
        .filter(|row| row["target"] == "codex_api::prompt_cache")
        .collect();
    assert_eq!(observations.len(), 3);
    for (id, state, cached) in [
        ("resp_http", "positive", Some(1024)),
        ("resp_ws_full", "zero", Some(0)),
        ("resp_ws_delta", "absent", None),
    ] {
        let row = observations
            .iter()
            .find(|row| row["fields"]["response_id"] == id)
            .unwrap();
        assert_eq!(row["instance_id"], instance);
        assert_eq!(row["fields"]["input_tokens"], 2048);
        assert_eq!(row["fields"]["cached_state"], state);
        assert_eq!(
            row["fields"].get("cached_tokens").and_then(Value::as_u64),
            cached
        );
    }
    let wire: Vec<_> = rows
        .iter()
        .filter(|row| row["target"] == "codex_api::prompt_cache_wire")
        .collect();
    if wire_enabled {
        let requests: Vec<_> = wire
            .iter()
            .filter(|row| row["fields"].get("mode").is_some())
            .collect();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().any(|row| row["fields"]["mode"] == "delta"));
        for row in requests {
            assert_eq!(row["instance_id"], instance);
            let fields = &row["fields"];
            assert_eq!(fields["logical_prefix"], "unknown");
            assert!(fields["warmup"].is_boolean());
            let hash = fields["model_hash"].as_str().unwrap();
            assert_eq!(hash.len(), 16);
            assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
            let ordinal = fields["wire_ordinal"].as_u64().unwrap();
            assert_eq!(
                wire.iter()
                    .filter(|event| event["fields"]["wire_ordinal"] == ordinal
                        && event["fields"]["stage"] == "completed")
                    .count(),
                1
            );
        }
    } else {
        assert!(wire.is_empty());
    }
    for summary in rows.iter().filter(|row| row["kind"] == "batch_summary") {
        assert_eq!(summary["run"]["instance_id"], instance);
        assert_eq!(summary["collector"]["dropped"], 0);
    }
    for (path, bytes) in batches {
        assert_eq!(std::fs::read(path)?, bytes);
        assert!(!String::from_utf8(bytes)?.contains("PRIVATE_"));
    }
    Ok(())
}
