# Official 0.165.0 local modifications

OpenDIY Codex is independent and unofficial. This source snapshot is based on upstream release `rust-v0.165.0`, commit `360319d8dee62d87e2d59fc0c21805d79519cfed`, with local changes frozen at `f665ccc64aaf6baee8fd137d8054f1daaae4a398`. The source version is `0.165.0+native-r26.20261028`. Upstream LICENSE, NOTICE and third-party notices are retained.

`upstream/official-0.165.0.patch` contains only the selected public source changes. `SOURCE_BASELINE.json` and `upstream/source-inventory.json` identify all 148 modified source/build-tool files and every exported Git blob. No local Git history is imported. The existing project README prose is retained with one current-source link added to each language; root LICENSE/NOTICE remain byte-for-byte unchanged. The r22 description is historical. This document and SOURCE_BASELINE identify the current source snapshot.

The changes cover MCP request cancellation and process ownership/cleanup, configuration and reload boundaries, capacity retry/trace ownership, background terminal startup yielding, cache observation, scheduled TUI work, terminal controls and related regression tests. `tools/windows-cli` contains packaging source and its disposable test fixture. This is a source export, not an installable desktop release or a claim that live-app latency, resource growth or all regressions are resolved.

The root .gitattributes preserves the public repository's existing rules and merges the pinned source rules for generated schemas and LF source/build inputs; checks/.gitattributes is excluded. This merged source input is included in the 148-file modification/provenance list.

Modified Rust, TOML/lock, Python, Markdown and tool configuration files carry a dated OpenDIY modification comment. `sourceSha256` records the pinned Git blob; `publishedSha256` records canonical published Git bytes, including those comments and the root attributes merge. The published bytes therefore differ from the private build input and are not claimed to reproduce a previously built executable. Inventory hashes describe committed Git bytes, not every physical Windows checkout byte. For the 38 recorded embedded prompt inputs, `upstream/windows-prompt-inputs.json` separately records canonical Git-blob SHA-256 and the pinned Windows source-input SHA-256. Only those paths override the existing `* -text` rule with native `text` EOL handling; Linux is not forced to CRLF. On Windows, the verifier checks the actual raw prompt bytes against the Windows hashes before comparing normalized Git bytes. These 38 inputs match the pinned private source input; this is not a whole-binary reproducibility claim. For generated TypeScript/JSON, compressed schema resources and snapshots, the data remains byte-exact and the following list is the external modification notice. Generated TypeScript must keep the actual GENERATED_TS_HEADER at byte zero: the original Rust schema comparison strips that exact prefix and compares the remaining serialized content. A prepended local notice breaks this contract even though it is a valid TypeScript comment. The exporter therefore leaves generated TypeScript exactly equal to the pinned source, without weakening the Rust comparison. Other data formats/snapshots also retain their original byte representation:

- `codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshParams.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshResponse.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadPluginReloadParams.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadPluginReloadResponse.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/TurnStartIfIdleParams.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/TurnStartIfIdleResponse.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/index.ts`
- `codex-rs/app-server-protocol/schema/json/ClientRequest.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`
- `codex-rs/app-server-protocol/schema/json/v2/McpServerRefreshResponse.json`
- `codex-rs/app-server-protocol/schema/json/v2/NullableMcpServerRefreshParams.json`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadPluginReloadParams.json`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadPluginReloadResponse.json`
- `codex-rs/app-server-protocol/schema/json/v2/TurnStartIfIdleParams.json`
- `codex-rs/app-server-protocol/schema/json/v2/TurnStartIfIdleResponse.json`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_custom.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_fast_mode_enabled.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_first_actions.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_global_chord.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_narrow.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_wide.snap`

The tracked upstream workflow `codex-rs/vendor/bubblewrap/.github/workflows/check.yml` is intentionally excluded by the existing export filter: this import does not enable upstream automation. The exact exclusion and reason remain in `upstream/source-inventory.json`; `checks/.gitattributes` and operational checks remain excluded as well.

These generated resources remain source/build/test inputs under their existing notices. Executables, packages, operational checks, logs, credentials, profiles, receipts and private desktop assets are excluded. Existing synthetic AWS credential fixtures are public test data inherited from upstream, not credentials to any account; they remain unchanged.

## Historical r22 record

The following earlier record and the original `upstream/r22.patch` and build-input hash catalog are retained for project history. They describe r22, not the current snapshot.

# r22 local modifications

OpenDIY Codex is an independent project. The upstream Apache-2.0 license and copyright notices are retained; upstream LICENSE and NOTICE are also preserved under `upstream/`.

The official source baseline is `6905ecede7816e91373e9eabda3f6b0368fc747b`. Local r22 changes are frozen at `f74c7574ae39b95550d6b445f828ba1010787f74`. The exact original change is `upstream/r22.patch`; its SHA-256 and the 23 changed files are listed in `SOURCE_BASELINE.json`.

The r22 change set covers MCP read lifetime/cancellation handling, Windows stdio and child/Job lifetime handling, related regression tests, and dependency lock updates. This description identifies the source change set and does not claim that a public release or all runtime behavior has been validated.

On 2026-10-26, a prominent one-line OpenDIY Codex modification notice was added to each of these 23 files using the file format's comment syntax (`//` for Rust and `#` for Cargo TOML/lock files). Existing copyright and license text was retained. Apart from this notice prefix, published source bytes equal the frozen Git blob. No uncommitted r23 source changes were imported.

`sourceSha256` identifies the frozen Git blob bytes; `publishedSha256` includes the notice prefix. `builtSourceSha256` separately identifies the recorded r22 Windows build input, which uses CRLF in 19 of the files. These hashes intentionally describe different byte representations. The published tree is not presented as byte-identical to the old build input or as a reproduced binary.

Unmodified third-party source remains under its existing license. `upstream/NOTICE` includes the original Ratatui attribution. The vendored bubblewrap license and copyright notices, including its `COPYING` text, are preserved unchanged.
