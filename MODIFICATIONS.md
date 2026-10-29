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

## R27 work branch — 2026-10-29

Source implementation: `10d9ef1e0d80f379f4c0fc1dc343924f609dae66`. Adds opt-in buffered diagnostics, thread-owned context limits and Korean UI. This work branch is under integration testing; it is not a verified release or installation. Generated protocol fixtures are regenerated from the changed Rust definitions. Existing upstream notices and licenses are retained.

Changed source and generated fixture files:

- `codex-rs/Cargo.lock`
- `codex-rs/Cargo.toml`
- `codex-rs/app-server-protocol/schema/json/ServerNotification.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadSettingsUpdatedNotification.json`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadSettings.ts`
- `codex-rs/app-server-protocol/src/export.rs`
- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/tests.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`
- `codex-rs/app-server/src/lib.rs`
- `codex-rs/app-server/src/request_processors/persisted_resume_settings_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_processor_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_summary.rs`
- `codex-rs/app-server/src/request_processors/turn_processor.rs`
- `codex-rs/app-server/src/thread_state.rs`
- `codex-rs/app-server/tests/suite/v2/thread_settings_update.rs`
- `codex-rs/cli/src/config_override_args.rs`
- `codex-rs/cli/src/localization.rs`
- `codex-rs/cli/src/main.rs`
- `codex-rs/core/src/agent/control_tests.rs`
- `codex-rs/core/src/codex_thread.rs`
- `codex-rs/core/src/session/handlers.rs`
- `codex-rs/core/src/session/mod.rs`
- `codex-rs/core/src/session/session.rs`
- `codex-rs/core/src/session/step_activation.rs`
- `codex-rs/core/src/session/step_activation_tests.rs`
- `codex-rs/core/src/session/tests.rs`
- `codex-rs/core/src/session/thread_settings.rs`
- `codex-rs/core/src/session/turn_context.rs`
- `codex-rs/core/src/session/turn_input.rs`
- `codex-rs/core/tests/suite/settings_constraints.rs`
- `codex-rs/diagnostics/Cargo.toml`
- `codex-rs/diagnostics/src/batch_log.rs`
- `codex-rs/diagnostics/src/lib.rs`
- `codex-rs/exec/Cargo.toml`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/state/src/extract.rs`
- `codex-rs/thread-store/src/thread_metadata_sync.rs`
- `codex-rs/tui/Cargo.toml`
- `codex-rs/tui/src/app/app_server_event_targets.rs`
- `codex-rs/tui/src/app/event_dispatch.rs`
- `codex-rs/tui/src/app/tests.rs`
- `codex-rs/tui/src/app/thread_settings.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app_server_session.rs`
- `codex-rs/tui/src/bottom_pane/approval_overlay.rs`
- `codex-rs/tui/src/bottom_pane/experimental_features_view.rs`
- `codex-rs/tui/src/bottom_pane/footer.rs`
- `codex-rs/tui/src/bottom_pane/popup_consts.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/model_popups.rs`
- `codex-rs/tui/src/chatwidget/permissions_menu.rs`
- `codex-rs/tui/src/chatwidget/settings.rs`
- `codex-rs/tui/src/chatwidget/settings_popups.rs`
- `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- `codex-rs/tui/src/chatwidget/tests/app_server.rs`
- `codex-rs/tui/src/chatwidget/tests/slash_commands.rs`
- `codex-rs/tui/src/config_panel/config_panel_tests.rs`
- `codex-rs/tui/src/config_panel/mod.rs`
- `codex-rs/tui/src/config_panel/render.rs`
- `codex-rs/tui/src/i18n.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/onboarding/auth.rs`
- `codex-rs/tui/src/onboarding/auth/headless_chatgpt_login.rs`
- `codex-rs/tui/src/onboarding/trust_directory.rs`
- `codex-rs/tui/src/onboarding/welcome.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/startup_orchestration.rs`
- `codex-rs/tui/src/status_indicator_widget.rs`
- `docs/native-cli-r27.ko.md`

Source update `da5893f12d012899c29336db426868d94942ec3a` (integration in progress):

- `codex-rs/app-server/src/lib.rs`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/tui/src/startup_orchestration.rs`

Source update `012b105ff548caa7bbd64e67e6fb86d192320077` (integration in progress):

- `tools/windows-cli/README.md`
- `tools/windows-cli/package_cli.py`
- `tools/windows-cli/test_package_cli.py`

Source update `6b4628988df39ae5995f0f3706687d73e9d89ea9` (integration in progress):

- `codex-rs/Cargo.lock`
- `codex-rs/Cargo.toml`
- `codex-rs/app-server-daemon/Cargo.toml`
- `codex-rs/app-server-daemon/src/lib.rs`
- `codex-rs/app-server-daemon/src/remote_control_client.rs`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts`
- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/memory.rs`
- `codex-rs/app-server-transport/src/transport/remote_control/tests.rs`
- `codex-rs/app-server/src/main.rs`
- `codex-rs/app-server/src/main_tests.rs`
- `codex-rs/app-server/src/message_processor.rs`
- `codex-rs/app-server/src/request_processors/persisted_resume_settings_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_memory.rs`
- `codex-rs/app-server/tests/suite/v2/memory_read.rs`
- `codex-rs/app-server/tests/suite/v2/remote_control.rs`
- `codex-rs/cli/src/localization.rs`
- `codex-rs/cli/src/main.rs`
- `codex-rs/cli/src/remote_control_cmd.rs`
- `codex-rs/cloud-tasks/src/cli.rs`
- `codex-rs/cloud-tasks/src/env_detect.rs`
- `codex-rs/cloud-tasks/src/env_detect_tests.rs`
- `codex-rs/cloud-tasks/src/lib.rs`
- `codex-rs/cloud-tasks/src/util.rs`
- `codex-rs/config/src/lib.rs`
- `codex-rs/config/src/memory_scope.rs`
- `codex-rs/config/src/types.rs`
- `codex-rs/core/src/agent/control_tests.rs`
- `codex-rs/core/src/config/config_tests.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/session/mod.rs`
- `codex-rs/core/src/session/session.rs`
- `codex-rs/ext/memories/src/extension.rs`
- `codex-rs/ext/memories/src/prompts.rs`
- `codex-rs/ext/memories/src/prompts_tests.rs`
- `codex-rs/ext/memories/src/tests.rs`
- `codex-rs/ext/memories/templates/memories/read_path.md`
- `codex-rs/ext/memories/templates/memories/read_path_v2.md`
- `codex-rs/external-agent-migration/src/memory.rs`
- `codex-rs/external-agent-migration/src/memory_import.rs`
- `codex-rs/external-agent-migration/src/memory_import_tests.rs`
- `codex-rs/external-agent-migration/src/memory_tests.rs`
- `codex-rs/external-agent-migration/src/sessions/records_cla.rs`
- `codex-rs/memories/write/src/lib.rs`
- `codex-rs/memories/write/src/notes.rs`
- `codex-rs/memories/write/src/phase1.rs`
- `codex-rs/memories/write/src/phase2.rs`
- `codex-rs/memories/write/src/rollout_input.rs`
- `codex-rs/memories/write/src/runtime.rs`
- `codex-rs/memories/write/src/start.rs`
- `codex-rs/memories/write/src/startup_tests.rs`
- `codex-rs/protocol/src/memory_version.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/state/src/extract.rs`
- `codex-rs/state/src/runtime/memories.rs`
- `codex-rs/state/src/runtime/memory_versions.rs`
- `codex-rs/state/src/runtime/memory_versions_tests.rs`
- `codex-rs/state/src/sqlite.rs`
- `codex-rs/state/src/sqlite_tests.rs`
- `codex-rs/thread-store/src/thread_metadata_sync.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/app/agent_navigation.rs`
- `codex-rs/tui/src/app/agent_picker.rs`
- `codex-rs/tui/src/app/config_persistence.rs`
- `codex-rs/tui/src/app/event_dispatch.rs`
- `codex-rs/tui/src/app/memory.rs`
- `codex-rs/tui/src/app/subagent_status_tests.rs`
- `codex-rs/tui/src/app/tests/local_command_scroll_tests.rs`
- `codex-rs/tui/src/app/thread_routing.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app_server_session.rs`
- `codex-rs/tui/src/bottom_pane/memories_settings_view.rs`
- `codex-rs/tui/src/bottom_pane/memories_settings_view_tests.rs`
- `codex-rs/tui/src/bottom_pane/mod.rs`
- `codex-rs/tui/src/bottom_pane/subagent_status.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- `codex-rs/tui/src/chatwidget/status_controls.rs`
- `codex-rs/tui/src/i18n.rs`
- `codex-rs/tui/src/slash_command.rs`

Source update `a7bd64a5888e7a6ccc58b175c4363584bd7ecc61` (integration in progress):

- `codex-rs/core/config.schema.json`
- `codex-rs/diagnostics/src/batch_log.rs`

Source update `5270c5df5701b71bc7ec9514a17838c8afcd22dc` (integration in progress):

- `codex-rs/cli/src/localization.rs`
- `codex-rs/tui/src/onboarding/trust_directory.rs`
- `codex-rs/tui/src/status_indicator_widget.rs`

소스 갱신 `b34fa5a1aab69b1e0044a23d34b153099b33813b` (통합 진행 중):

- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/tests/suite/v2/thread_read.rs`
- `codex-rs/core/src/session_rollout_init_error.rs`
- `codex-rs/core/src/session_rollout_init_error_tests.rs`
- `codex-rs/core/src/thread_manager/shutdown.rs`
- `codex-rs/thread-store/src/error.rs`
- `codex-rs/thread-store/src/lib.rs`
- `codex-rs/thread-store/src/local/thread_history/read.rs`
- `codex-rs/thread-store/src/local/thread_history/read_tests.rs`
- `codex-rs/thread-store/src/local/thread_history/segment_paging.rs`
- `codex-rs/thread-store/src/local/thread_history_materialization_tests.rs`

소스 갱신 `7af94cabdab420bc52f43c152b900bf345279aee` (통합 진행 중):

- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/src/request_processors/thread_processor_tests.rs`
- `codex-rs/thread-store/src/local/thread_history_materialization_tests.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/app/agent_message_consolidation.rs`
- `codex-rs/tui/src/app/event_dispatch.rs`
- `codex-rs/tui/src/app/history_pagination.rs`
- `codex-rs/tui/src/app/history_pagination_tests.rs`
- `codex-rs/tui/src/app/history_ui.rs`
- `codex-rs/tui/src/app/history_window.rs`
- `codex-rs/tui/src/app/owned_transcript.rs`
- `codex-rs/tui/src/app/session_lifecycle.rs`
- `codex-rs/tui/src/app/startup.rs`
- `codex-rs/tui/src/app/test_support.rs`
- `codex-rs/tui/src/app/tests.rs`
- `codex-rs/tui/src/app/tests/history_hydration_tests.rs`
- `codex-rs/tui/src/app/tests/pagination_completion_tests.rs`
- `codex-rs/tui/src/app/thread_event_buffer.rs`
- `codex-rs/tui/src/app/thread_event_buffer_tests.rs`
- `codex-rs/tui/src/app/thread_events.rs`
- `codex-rs/tui/src/app/thread_routing.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app_server_session.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/app_server_session/history_tests.rs`
- `codex-rs/tui/src/app_server_session/rollout_history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history_tests.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/activity_groups.rs`
- `codex-rs/tui/src/chatwidget/command_lifecycle.rs`
- `codex-rs/tui/src/chatwidget/dynamic_activity.rs`
- `codex-rs/tui/src/chatwidget/streaming.rs`
- `codex-rs/tui/src/chatwidget/tests/reasoning_status_tests.rs`
- `codex-rs/tui/src/chatwidget/tool_lifecycle.rs`
- `codex-rs/tui/src/chatwidget/transcript.rs`
- `codex-rs/tui/src/exec_cell/live_output.rs`
- `codex-rs/tui/src/exec_cell/model.rs`
- `codex-rs/tui/src/exec_cell/render.rs`
- `codex-rs/tui/src/history_cell/activity_details.rs`
- `codex-rs/tui/src/history_cell/base.rs`
- `codex-rs/tui/src/history_cell/computer_activity.rs`
- `codex-rs/tui/src/history_cell/computer_activity_tests.rs`
- `codex-rs/tui/src/history_cell/dynamic.rs`
- `codex-rs/tui/src/history_cell/mcp.rs`
- `codex-rs/tui/src/history_cell/mcp_result.rs`
- `codex-rs/tui/src/history_cell/messages.rs`
- `codex-rs/tui/src/history_cell/mod.rs`
- `codex-rs/tui/src/history_window.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/pager_overlay/transcript.rs`
- `codex-rs/tui/src/thread_transcript/computer_groups.rs`
- `codex-rs/tui/src/thread_transcript/exploration_groups.rs`
- `codex-rs/tui/src/transcript_view/layout.rs`
- `codex-rs/tui/src/transcript_view/snapshot.rs`

소스 갱신 `8dfe79b00aeb9f950a86f2baaad9ea81b45b8921` (통합 진행 중):

- `codex-rs/app-server/src/request_processors/thread_attachments.rs`
- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/src/request_processors/thread_processor_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_sections.rs`
- `codex-rs/app-server/tests/suite/v2/thread_read.rs`
- `codex-rs/core/src/session_rollout_init_error.rs`
- `codex-rs/core/src/session_rollout_init_error_tests.rs`
- `codex-rs/core/src/thread_manager/shutdown.rs`
- `codex-rs/thread-store/src/error.rs`
- `codex-rs/thread-store/src/lib.rs`
- `codex-rs/thread-store/src/local/thread_history/read.rs`
- `codex-rs/thread-store/src/local/thread_history/read_tests.rs`
- `codex-rs/thread-store/src/local/thread_history/segment_paging.rs`
- `codex-rs/tui/src/app/history_pagination.rs`
- `codex-rs/tui/src/app/history_window.rs`
- `codex-rs/tui/src/app/native_history.rs`
- `codex-rs/tui/src/app/native_history_tests.rs`
- `codex-rs/tui/src/app/tests/history_hydration_tests.rs`
- `codex-rs/tui/src/app/thread_event_buffer.rs`
- `codex-rs/tui/src/app/thread_event_buffer_tests.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/chatwidget/streaming.rs`
- `codex-rs/tui/src/chatwidget/tests/copy_export_picker_tests.rs`
- `codex-rs/tui/src/chatwidget/transcript.rs`
- `codex-rs/tui/src/history_window.rs`

소스 갱신 `d10723ca45f45db13539865ac13d6892660fb788` (통합 진행 중):

- `codex-rs/app-server-protocol/src/protocol/v2/mcp.rs`
- `codex-rs/core/src/session/step_activation_tests.rs`
- `codex-rs/ext/memories/src/prompts_tests.rs`
- `codex-rs/thread-store/src/local/thread_history_materialization_tests.rs`
