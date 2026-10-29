# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

> Because "fuck it" wasn't enough. I fixed it.

Codex CLI and Rust App Server patches for Windows: request cancellation, process cleanup, bounded chat display, and diagnostics that survive ordinary error exits.

The current source is **R28, based on official 0.165.0**.

- **Keep working:** bounded capacity retries, cancellation propagation, and owned-process cleanup.
- **Control each conversation:** context-window overrides, scoped memory, and a subagent status footer.
- **Keep the screen smaller:** reuse official history APIs; limit retained display history without trimming the model's original conversation.
- **Diagnose failures:** opt-in buffered logs, immutable batches, and retryable asynchronous export. Partial Korean interface included.

[Build from source](docs/source-import.md) · [CLI settings](docs/native-cli-r27.ko.md) · [R28 validation and limits](docs/cli-r28-validation.md) · [Changes](MODIFICATIONS.md)

[Agent Grid](https://github.com/jaeseongs95/agent-grid) owns the desktop interface; this repository owns the CLI and App Server. Real authenticated mobile access to the same backend is still unverified. No prompt-cache hit-rate gain or complete fix for memory, connection growth, or UI latency is claimed.

**Source and build tools only.** No installable binaries, private desktop assets, credentials, or operational logs are published. Independent, unofficial work based on [OpenAI Codex](https://github.com/openai/codex); see [Apache-2.0](LICENSE) and [third-party notices](NOTICE).
