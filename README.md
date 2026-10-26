# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

**Inspect the patch. Know what was tested.**

Windows-focused source patches for Codex CLI and Rust App Server.

## Why it matters

- **Follow cleanup and cancellation.** Inspect changes to Windows process/Job lifetimes, stdio, sockets and MCP request cancellation.
- **See the test boundary.** The r22 Windows candidate passed 18 targeted Cargo tests; the methods and unverified areas are documented.
- **Trace every change.** Compare the full Rust workspace against a pinned upstream revision and recorded source hashes.

## Explore

- [CLI source](codex-rs/cli) · [Rust App Server source](codex-rs/app-server)
- [Test methods and limits](docs/TESTING.md)
- [Source baseline](SOURCE_BASELINE.json) · [Modifications](MODIFICATIONS.md)

Historical r22 tests do not establish a fresh public-clone release build or full desktop validation.

Independent and unofficial, based on [OpenAI Codex](https://github.com/openai/codex). [Apache-2.0](LICENSE); applicable [third-party notices](NOTICE) remain in place.
Source only: no installable patched binary; proprietary desktop assets are not redistributed.
