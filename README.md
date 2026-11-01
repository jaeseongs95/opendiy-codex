# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

An unofficial Windows edition of **Codex CLI and Rust App Server**, with fixes for cancellation and process cleanup, per-conversation controls, and buffered diagnostics.

[Download R30](https://github.com/jaeseongs95/opendiy-codex/releases/tag/v0.165.0-r30.20261101) · [Build from source](docs/source-import.md) · [Validation](docs/TESTING.md) · [Changes](MODIFICATIONS.md)

## Versions

| Channel | Official base | Status |
| --- | --- | --- |
| **R30 release** | 0.165.0 | Windows x86-64 CLI, standalone App Server, and portable package available |
| **R31 source on main** | 0.166.0 | Integration and regression validation in progress; no R31 release yet |

Downloading R30 and building main produce different versions. [SOURCE_BASELINE.json](SOURCE_BASELINE.json) identifies the source snapshot; [Releases](https://github.com/jaeseongs95/opendiy-codex/releases) identifies published executables and their validation scope.

## What it adds

- **Cancellation and cleanup:** propagate cancellation through pending requests, limit retries for model-capacity errors, avoid replay after partial output, and clean up owned child processes.
- **Conversation controls:** set a context-window override, manage memory scope and corrections, and view subagent counts above the input field.
- **Scheduled work:** run prompts at intervals or at a specified time while the CLI stays open. Busy conversations skip that occurrence instead of accumulating work.
- **Smaller display history:** reuse official history APIs and cursors while limiting retained display history. The model's original conversation remains separate.
- **Optional diagnostics:** buffer events off the input path, write completed batches as separate files, and retry asynchronous export without appending to published files.
- **Language choice:** English by default, with a saved global Korean option. Commands and model responses are not translated automatically.

These features were introduced across R26–R30. R31 carries them onto official 0.166.0 and adds fixes for request/agent-loop lifetime, history-query connection use, stale client state, and cron language handling. R31's final build, integration checks, and release validation are still pending.

## Run on Windows

1. Download `opendiy-codex-r30-windows-x86_64.zip` and `SHA256SUMS.txt` from the [R30 release](https://github.com/jaeseongs95/opendiy-codex/releases/tag/v0.165.0-r30.20261101).
2. Verify the archive's SHA-256 and extract it into a new folder, keeping your existing installation.
3. Run `codex-local.cmd` to use the packaged CLI with its local engine, separate from an existing shared server. See the included `USAGE.ko.md` for package details.

The archive includes the CLI, standalone App Server, Windows helpers, licenses, and usage notes. Individual `codex.exe` and `codex-app-server.exe` downloads are also available; features requiring helpers need the full package. `codex-patched.cmd` and the default CLI may connect to an existing shared server, so the launched executable alone does not establish which backend is serving the conversation.

## Commands

Enter these in the CLI conversation input, not in PowerShell. The examples are available in R30; R31 changes remain subject to validation.

| Command | Purpose |
| --- | --- |
| `/cron add --every 10m -- Check whether the build has finished` | Schedule a repeating prompt |
| `/cron list` | Show this conversation's schedules |
| `/cron remove 1` | Remove schedule 1's future runs; use its actual returned ID |
| `/context 120000` · `/context reset` | Save or clear this conversation's context-window override |
| `/memory view` · `/memory edit <correction>` | View stored memory or record a correction for later consolidation |
| `/subagents` | View subagent conversations |
| `/background` or `F6` | Return control while a supported terminal task continues |
| `/reload_plugin` | Request plugin reload for the current conversation |
| `/language ko` · `/language en` | Save the global interface language for the next launch |

Context settings apply from the next turn, survive resume, and cannot raise the model's catalog limit. Memory corrections do not immediately rewrite the stored summary or existing chat context. `CODEX_UI_LANGUAGE` overrides the saved language.

Schedules disappear when the CLI exits. Natural-language scheduling works only when the conversation exposes the CLI's `cron` tool; an old resumed conversation may need direct commands or a new chat. See [scheduling and subagent display](docs/native-cli-cron.ko.md) and [context, language, and logging](docs/native-cli-r27.ko.md). These guides are currently in Korean.

## Diagnostics and validation

Logging is off unless explicitly enabled in `CODEX_HOME/cli-logging.toml`. Categories, buffering, and an optional export destination are configurable. Export success means filesystem publication, not confirmed cloud upload. Prompt text and authentication headers are excluded from these diagnostic batches.

[Validation records](docs/TESTING.md) distinguish candidates, tests, and remaining checks. Display retention is not a total process-memory limit: legacy conversations can still use the official full-history fallback. Authenticated mobile access to the same backend and sustained workload behavior remain unverified. No prompt-cache hit-rate improvement or complete resolution of memory growth, connection accumulation, or desktop latency is claimed.

## Two projects, one backend

| Project | Responsibility |
| --- | --- |
| **OpenDIY Codex** | CLI, Rust App Server, protocol changes, and build tools |
| [**OpenDIY Desktop**](https://github.com/jaeseongs95/opendiy-desktop) | Desktop interface and multi-session management |

The projects coordinate around the same backend. This repository does not distribute private ChatGPT desktop assets. Source and build tools live in Git; executable packages are published separately in Releases. Credentials and operational logs are excluded.

## Build and license

See the [source and build guide](docs/source-import.md) for the pinned toolchain, source verification, and CLI/App Server build commands, and the [Windows packaging tools](tools/windows-cli/README.md). Source-export verification does not establish that a fresh public checkout has completed a full build or runtime validation.

Independent work based on [OpenAI Codex](https://github.com/openai/codex), not an official OpenAI product. See [Apache-2.0](LICENSE), [NOTICE](NOTICE), and component-specific licenses; third-party components retain their original licenses.
