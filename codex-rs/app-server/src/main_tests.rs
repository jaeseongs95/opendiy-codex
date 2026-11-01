// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
use super::AppServerArgs;
use clap::Parser;
use codex_app_server::AppServerTransport;
use pretty_assertions::assert_eq;
use toml::Value as TomlValue;
use url::Url;

#[test]
fn app_server_remote_control_startup_flags_are_exclusive_and_unselected_by_default() {
    let defaults = AppServerArgs::try_parse_from(["codex-app-server"]).expect("default args");
    assert!(!defaults.remote_control);
    assert!(!defaults.no_remote_control);
    let disabled = AppServerArgs::try_parse_from(["codex-app-server", "--no-remote-control"])
        .expect("process-scoped disable");
    assert!(disabled.no_remote_control);
    assert!(!disabled.remote_control);
    let enabled = AppServerArgs::try_parse_from(["codex-app-server", "--remote-control"])
        .expect("process-scoped enable");
    assert!(enabled.remote_control);
    assert!(!enabled.no_remote_control);
    for flags in [
        ["--remote-control", "--no-remote-control"],
        ["--no-remote-control", "--remote-control"],
    ] {
        let error = AppServerArgs::try_parse_from(["codex-app-server", flags[0], flags[1]])
            .expect_err("startup flags must conflict");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }
}

#[test]
fn app_server_accepts_cli_config_overrides() {
    let args = AppServerArgs::try_parse_from([
        "codex-app-server",
        "-c",
        "model=\"gpt-5-codex\"",
        "--config",
        "sandbox_mode=\"read-only\"",
        "--listen",
        "off",
    ])
    .expect("parse app-server args");

    let parsed_overrides = args
        .config_overrides
        .parse_overrides()
        .expect("parse config overrides");

    assert_eq!(
        parsed_overrides,
        vec![
            (
                "model".to_string(),
                TomlValue::String("gpt-5-codex".to_string()),
            ),
            (
                "sandbox_mode".to_string(),
                TomlValue::String("read-only".to_string()),
            ),
        ]
    );
}

#[test]
fn app_server_accepts_process_scoped_grpc_code_mode_host() {
    let args = AppServerArgs::try_parse_from([
        "codex-app-server",
        "--code-mode-host",
        "https://example.test",
        "--listen",
        "off",
    ])
    .expect("parse gRPC app-server args");

    assert_eq!(
        args.code_mode_host.code_mode_host,
        Some(Url::parse("https://example.test").expect("test endpoint should parse"))
    );
    assert_eq!(args.listen, AppServerTransport::Off);
}

#[test]
fn app_server_rejects_invalid_code_mode_host() {
    for endpoint in [
        "ftp://127.0.0.1:8765",
        "ws://",
        "ws://127.0.0.1:8765",
        "wss://example.test/code-mode",
        "ws://alice:secret@example.test/code-mode",
        "wss://alice:secret@example.test/code-mode",
        "wss://example.test/code-mode#fragment",
        "http://",
        "https://example.test/#fragment",
        "https://example.test/code-mode",
        "http://alice:secret@example.test",
        "https://alice:secret@example.test",
        "http://example.test/?token=secret",
    ] {
        let error =
            AppServerArgs::try_parse_from(["codex-app-server", "--code-mode-host", endpoint])
                .expect_err("invalid code-mode host endpoint should fail startup argument parsing");

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        let rendered_error = error.to_string();
        assert!(!rendered_error.contains("alice"));
        assert!(!rendered_error.contains("secret"));
    }
}
