# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

Current source: [official 0.165.0 + native CLI patch](docs/source-import.md) · [testing scope and remaining limits](docs/TESTING.md). The r22 overview below is retained as project history.

A fork of Codex CLI and Rust App Server that addresses stalled requests and child-process cleanup on Windows.

The published r22 patches cover MCP request cancellation, server shutdown and waits, and configuration reads.

## What changes

- Clean up work owned by a cancelled MCP read request, including its private connections and processes, before returning the cancellation response.
- Continue cleanup until directly owned MCP processes have actually exited, and wait for the previous process to be cleaned up before retrying a launch.
- Avoid unnecessarily occupying filesystem I/O threads while waiting for a quiet MCP server.
- Remove a redundant configuration load so the configuration response and feature settings use values from the same point in time.

## Source and documentation

- [CLI source](codex-rs/cli) · [App Server source](codex-rs/app-server) · [Source layout and building](docs/source-import.md)
- [Modifications](MODIFICATIONS.md) · [Testing methods and limitations](docs/TESTING.md)

An independent, unofficial project based on [OpenAI Codex](https://github.com/openai/codex), under [Apache-2.0](LICENSE) and the applicable [third-party notices](NOTICE).

Only r22 source is provided; installable patched binaries and proprietary desktop UI assets are not included. Full desktop validation and complete resolution of memory leaks or UI latency have not been established.
