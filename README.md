# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

An unofficial Windows edition of Codex CLI and Rust App Server. It repairs confirmed defects and reuses upstream functionality rather than duplicating it.

[Download latest release](https://github.com/jaeseongs95/opendiy-codex/releases/latest) · [Build from source](docs/source-import.md) · [Validation](docs/TESTING.md) · [Changes](MODIFICATIONS.md)

## Release status

| Version | Official base | State |
| --- | --- | --- |
| R39 | 0.167.0 | Compact-only development candidate based on frozen R38. It changes the summary policy and prevents completed task messages from being reattached after compaction. Tests are in progress; final binaries and release publication are pending. Summary quality with a real model is unverified. See [scope and limits](docs/native-cli-r39.ko.md). |
| R38 | 0.167.0 | [Published and frozen (`v0.167.0-r38.20261106`)](https://github.com/jaeseongs95/opendiy-codex/releases/tag/v0.167.0-r38.20261106). See the tagged release notes for final package validation. Publication does not establish the version installed on a PC. |
| R37 | 0.166.0 + selected upstream changes | Published and frozen (`v0.166.0-r37.20261104`). Source pinned to `2289ae3aa847cd64e7e987f56180160ce67a8694`: daemon version selection guards, scoped memory writes and consolidation, shared language preferences, plugin cache handling, replay filtering, selectable subagent footer and input-dispatch fixes. Development tests include 81 input regressions and four Desktop submission/save/restart-restore scenarios in English and Korean UI. Managed Windows memory policy and the unsupported busy connection-close scenario remain outside that verified scope. Tagged release notes report final package verification; a source checkout or development test is not an installed release. |
| R36 | 0.166.0 + selected upstream changes | Published stable Windows release integrating R35 changes. Frozen after publication. |
| R35 | 0.166.0 | Integrated into R36 without a separate release. |
| R34 | 0.166.0 + selected upstream changes | Published Windows CLI, standalone App Server, and full package. Frozen after publication. |
| R33 | 0.166.0 | Historical package; Windows updater defects were repaired in R34. |
| R32 | 0.166.0 | Historical package. |
| R31 | 0.166.0 | No separate release; integrated into R32. |

Published Windows packages and checksums are listed in [Releases](https://github.com/jaeseongs95/opendiy-codex/releases). Source merges and development builds do not establish publication or the version installed on a PC. Check the tagged release notes for final binary validation and verify the executable path and version after installation. The [R35 guide](docs/native-cli-r35.ko.md) describes the memory, language, and update changes integrated into R36.

## Features

- Cancellation propagation, bounded capacity retries, and owned-child process cleanup.
- Per-conversation context/tier controls, memory activation/corrections, subagent display, and confirmed cleanup.
- CLI-lifetime scheduling: busy conversations skip an occurrence; schedules disappear when the CLI exits.
- R36: one-line subagent summary with at most three expanded rows, distinct Cloud discovery errors, read-only `codex ci` observation, and fast propagation/compaction phase diagnostics.
- Bounded display history independent of model context, and globally selected English/Korean UI.
- Optional asynchronous diagnostics published as completed immutable batch files.
- A package launcher/updater that retains previous packages and does not automatically replace an active shared daemon.

The changes integrated into R36 distinguish a server's optional capability declaration from a successful attachment or completed model turn. Its cache change fixes missing completed-usage observations; it does not change cache policy or establish a better live hit rate.

## Run on Windows

1. Download the complete ZIP and `SHA256SUMS.txt` from the release.
2. Verify SHA-256 and follow the package's `USAGE.ko.md`; retain your existing installation.
3. Use `codex-local.cmd` for an engine separate from an existing shared server.

The complete ZIP is recommended: extract it with its directory layout intact. An individual `codex.exe` or `codex-app-server.exe` is not a complete package. To use code-mode tools (including `functions`), place the matching release's `codex-code-mode-host.exe` in the **same folder as the executable being run**, keeping that exact filename. If it is missing, code-mode host startup can fail with `os error 2`.

R38 did not provide the helper as an individual release asset; obtain it from the R38 complete ZIP. R39 is planned to add `codex-code-mode-host.exe` as a separate asset; its publication is pending. Voice and other additional resources still require the complete ZIP. The default CLI may attach to an existing server: verify its identity and that server's helper location separately from the selected CLI.

## Conversation commands

These commands go in the CLI conversation input, not PowerShell.

| Command | Purpose |
| --- | --- |
| `/cron add --every 10m -- Check whether the build has finished` | Repeating prompt |
| `/cron list` · `/cron remove 1` | List/remove the actual schedule ID |
| `/context 120000` · `/context reset` | Conversation context override |
| `/memory view` · `/memory enable on` | Inspect memory / explicitly enable it |
| `/subagents` | Inspect child sessions |
| `/background` or `F6` | Background a supported terminal task |
| `/reload_plugin` | Request plugin reload |
| `/language ko` · `/language en` | Save global language for next launch |

Context overrides cannot exceed the model limit. Saving a memory correction is not immediate summary consolidation. Natural-language controls require the tools supplied by the running CLI version. Plugin reload acknowledgement does not prove every MCP server is ready.

Updater commands run in PowerShell: `codex update --check`, `--status`, `--enable-updates`, `--disable-updates`, or `codex update`. They require a supported installed package; see [updater requirements](tools/windows-cli/UPDATE.ko.md).

## OpenDIY Desktop compatibility

[OpenDIY Desktop](https://github.com/jaeseongs95/opendiy-desktop) owns the desktop UI and multi-session workflows. This repository owns the CLI, App Server, and their package. The projects coordinate protocol and release compatibility.

**Minimum for OpenDIY extensions in Desktop 1.1.0: OpenDIY CLI R36 and App Server R36.** The published R36 version is `0.166.0+native-r36.20261103`; the complete package from the same release is recommended. Versions before R36, including R28, are outside the supported range. Official Codex CLI 0.166.0 has bounded basic-function compatibility only; OpenDIY extensions are not guaranteed.

Existing servers, stored data, and in-progress work are preserved without automatic replacement. This preservation policy is separate from the supported-version range. The isolated tests below record verification history; R28 preservation checks do not establish official R28 support.

| CLI/server | Desktop | Verification scope |
| --- | --- | --- |
| R33 | 1.0.2 | Historical bounded joint tests; see the R33 release record. |
| R34 | Release-specific tested candidate | Startup, isolated engine/catalog, and restoration checks; see the R34 release record. |
| R36 trial CLI/server | 1.1.0 candidate `5eda4e5` | Isolated GUI input, Korean IME/paste, HTTP/SSE, same-backend completion, normal exit, and restart/restore checks passed. These results apply to the trial binaries named in this row. |
| Official Codex CLI 0.166.0 | Same 1.1.0 candidate | Six isolated basic-function checks passed. OpenDIY extensions are not guaranteed. |
| R36 trial CLI/isolated R28 server | Same 1.1.0 candidate | Seven isolated preservation checks passed. Historical test coverage only; R28 is outside the supported range. |

A connection, initialize response, or build-string match alone is not proof of full compatibility. Actual mobile connection, sustained workloads, and live cache effects need separate evidence. File creation does not prove cloud upload, and display-history limits are not total process-memory caps.

## Build and license

[Source build guide](docs/source-import.md) · [Windows packaging](tools/windows-cli/README.md). Git stores source and build tools; Releases stores executable assets. Source provenance is not a fresh public-checkout build. Credentials and operational logs are excluded.

Independent work based on [OpenAI Codex](https://github.com/openai/codex), not an official OpenAI product. [Apache-2.0](LICENSE) · [NOTICE](NOTICE).
