# Official 0.165.0 local modifications

OpenDIY Codex is independent and unofficial. This source snapshot is based on upstream release `rust-v0.165.0`, commit `360319d8dee62d87e2d59fc0c21805d79519cfed`, with local changes frozen at `159017bd9b04533865ee9cb8173d7efe41cb414e`. The source version is `0.165.0+native-r28.20261029`. Upstream LICENSE, NOTICE and third-party notices are retained.

## R29 선별 이식 후보

R29 준비 후보 `159017bd9b04533865ee9cb8173d7efe41cb414e`는 공식 0.165.0 기반을 유지하면서 공식 0.166 변경 5개만 선별 이식했다. 전체 0.166로 기반을 바꾸거나 실행 파일 버전을 0.166으로 올린 후보가 아니다. 이전 `67404f4` 이후 32파일이 바뀌었고 공식 0.165.0 대비 수정 목록은 394파일이다. 원래 수정 고지는 보존하며 이 후속 범위는 여기서 별도로 고지한다. 스키마 재생성 시험 1개와 표적 단위시험 컴파일은 종료 코드 0이었다. 같은 소스의 단위시험 11그룹은 802개 통과·0개 실패였고 컴파일 전후 원본 입력은 같았다. 현재 `core` 통합시험 컴파일이 진행 중이며 새 후보의 실행 파일·설치·독립감사는 미완료다. 실제 캐시율 개선을 측정한 결과는 아니다.

| 공식 변경 | 로컬 이식 | 범위 |
| --- | --- | --- |
| `eb3572f9d` | `b7151747a` | 도구 제안에서 불필요한 연결 도구 디렉터리 작업 감소 |
| `d93071fae` | `b900a85c4` | HTTP 응답 스트림에서 불필요한 출력 항목 보유 감소 |
| `f37aa4b1c` | `ce697e7b8` | 설정 처리의 불필요한 작업 감소 |
| `04e365c69` | `6a405165c` | 스트림 준비 뒤 샘플링 프롬프트 보유 해제 |
| `c62e9df39` | `159017bd9` | 재개·분기 때 추론 토큰 회계 보존 |

`upstream/official-0.165.0.patch` contains only the selected public source changes. `SOURCE_BASELINE.json` and `upstream/source-inventory.json` identify all 394 modified source/build-tool files and every exported Git blob. No local Git history is imported. The existing project README prose is retained with one current-source link added to each language; root LICENSE/NOTICE remain byte-for-byte unchanged. The r22 description is historical. This document and SOURCE_BASELINE identify the current source snapshot.

The changes cover MCP request cancellation and process ownership/cleanup, configuration and reload boundaries, capacity retry/trace ownership, background terminal startup yielding, cache observation, scheduled TUI work, terminal controls and related regression tests. `tools/windows-cli` contains packaging source and its disposable test fixture. This is a source export, not an installable desktop release or a claim that live-app latency, resource growth or all regressions are resolved.

아래 예약 도구 연결·설치 기록은 이전 고정 소스 `67404f4`의 이력입니다. 새 R29 후보의 설치·실행 결과가 아닙니다. 기존 R28 실행 파일은 보존하고 사용자 승인 후 새 패키지의 명령줄 바로가기·기본 공유 서버로 전환했습니다. 실제 모델이 자연어 요청에서 도구를 선택하는 평가는 실행하지 않았습니다. 기존 예약 실행기를 재사용하고 취소·중복 전달 경계를 보강하는 변경과 사용 안내는 [CLI 예약 실행](docs/native-cli-cron.ko.md)에 정리했습니다. 이 설치 전환을 기능 전체의 검증·운영 배포 완료로 해석하면 안 됩니다.

The root .gitattributes preserves the public repository's existing rules and merges the pinned source rules for generated schemas and LF source/build inputs; checks/.gitattributes is excluded. This merged source input is included in the 394-file modification/provenance list.

Modified Rust, TOML/lock, Python and tool configuration files carry a dated OpenDIY modification comment. Markdown notices are limited to documentation under `docs/`; runtime prompt templates keep their exact source bytes. `sourceSha256` records the pinned Git blob; `publishedSha256` records canonical published Git bytes, including those comments and the root attributes merge. The published bytes therefore differ from the private build input and are not claimed to reproduce a previously built executable. Inventory hashes describe committed Git bytes, not every physical Windows checkout byte. For the 38 recorded embedded prompt inputs, `upstream/windows-prompt-inputs.json` separately records canonical Git-blob SHA-256 and the pinned Windows source-input SHA-256. Only those paths override the existing `* -text` rule with native `text` EOL handling; Linux is not forced to CRLF. On Windows, the verifier checks the actual raw prompt bytes against the Windows hashes before comparing normalized Git bytes. These 38 inputs match the pinned private source input; this is not a whole-binary reproducibility claim. For generated TypeScript/JSON, compressed schema resources and snapshots, the data remains byte-exact and the following list is the external modification notice. Generated TypeScript must keep the actual GENERATED_TS_HEADER at byte zero: the original Rust schema comparison strips that exact prefix and compares the remaining serialized content. A prepended local notice breaks this contract even though it is a valid TypeScript comment. The exporter therefore leaves generated TypeScript exactly equal to the pinned source, without weakening the Rust comparison. Other data formats/snapshots also retain their original byte representation:

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

소스 갱신 `11776c706192892fa932db3bca7cc2d8d5b4ecb6` (통합 진행 중):

- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshResponse.ts`

소스 갱신 `68eb12b9f5eaaf084ec6cf9d97809394d27f714a` (통합 진행 중):

- `codex-rs/tui/src/app/agents_overview_tests.rs`
- `codex-rs/tui/src/app/history_pagination.rs`
- `codex-rs/tui/src/app/snapshots/codex_tui__app__daemon_menu__tests__daemon_cli_confirmation.snap`
- `codex-rs/tui/src/app/snapshots/codex_tui__app__owned_transcript__empty_state_animation_tests__fresh_thread_header.snap`
- `codex-rs/tui/src/app/snapshots/codex_tui__app__tests__app_server_thread_replacement_clears_previous_transcript_before_replay-2.snap`
- `codex-rs/tui/src/app/tests.rs`
- `codex-rs/tui/src/app/tests/history_hydration_tests.rs`
- `codex-rs/tui/src/app/tests/local_command_scroll_tests.rs`
- `codex-rs/tui/src/app/tests/safety_buffering.rs`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__active_reconnect__unavailable_conversation.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_buffered_replay_reconciles_captions.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_completed_after_thread_switch.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_partial_completed_after_thread_switch.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_replay_reconciles_matching_captions.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__session_lifecycle_requests__new_session_tests__widget_replacement_preserves_mention_usage_with_history_disabled.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__startup_frame_tests__startup_tip_in_transcript.snap`
- `codex-rs/tui/src/app/thread_events.rs`
- `codex-rs/tui/src/app/thread_routing.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history_tests.rs`
- `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__memories_settings_popup.snap`
- `codex-rs/tui/src/chatwidget/tests/slash_commands.rs`
- `codex-rs/tui/src/history_cell/snapshots/codex_tui__history_cell__tests__pnpm_update_available_history_cell_snapshot.snap`
- `codex-rs/tui/src/history_cell/snapshots/codex_tui__history_cell__tests__standalone_unix_update_available_history_cell_snapshot.snap`
- `codex-rs/tui/src/history_cell/snapshots/codex_tui__history_cell__tests__standalone_windows_update_available_history_cell_snapshot.snap`
- `codex-rs/tui/src/pets/mod.rs`
- `codex-rs/tui/src/snapshots/codex_tui__app__agents_overview__tests__agents_overview_directory_grouping.snap`
- `codex-rs/tui/src/snapshots/codex_tui__app__tests__selected_unloaded_subagent_V1.snap`
- `codex-rs/tui/src/snapshots/codex_tui__startup_draft__layout__tests__owned_startup_layout.snap`
- `codex-rs/tui/src/snapshots/codex_tui__update_prompt__tests__update_picker_selected_28x12.snap`
- `codex-rs/tui/src/snapshots/codex_tui__update_prompt__tests__update_prompt_modal.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_server_auth_not_required.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_cached_limits_hide_credits_without_flag.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_credits_and_limits.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_enterprise_monthly_credit_limit.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_forked_from.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_monthly_limit.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_reasoning_details.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_active_user_defined_profile.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_auto_review_permissions.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_chatgpt_plan_without_email.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_missing_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_refreshing_limits_notice.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_stale_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_unavailable_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_truncates_halfwidth_kana_in_narrow_terminal.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_truncates_in_narrow_terminal.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_uses_default_reasoning_when_config_empty.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_uses_generic_limit_labels_for_unsupported_windows.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__transcript_overlay_status_rate_limit_refresh.snap`

소스 갱신 `d686f8d2d2eec5d4f625a90709efec209a0adee7` (통합 진행 중):

- `codex-rs/app-server-daemon/src/remote_control_client.rs`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history_tests.rs`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_server_auth_required.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_remote_server.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_wraps_enterprise_monthly_credit_details_in_narrow_terminal.snap`
- `docs/native-cli-r27.ko.md`

소스 갱신 `9766a8b7b8c4d56d7076367ddc12f2917a3df197` (통합 진행 중):

- `codex-rs/diagnostics/Cargo.toml`
- `codex-rs/diagnostics/src/batch_log.rs`
- `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs`
- `codex-rs/tui/src/snapshots/codex_tui__app__tests__selected_unloaded_subagent_V2.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_local_background_server.snap`
- `docs/native-cli-r27.ko.md`

## 공유 로컬 서버 재접속·기본 프록시 — 2026-10-30

이전 고정 소스 `9766a8b7b8c4d56d7076367ddc12f2917a3df197`는 기존 자연어 예약 연결에 다음 수정을 더했다. 고정 원본 소스의 표적 시험·전체 TUI 시험과 일반 개발 실행 파일의 제한된 격리 시험을 완료했다. 독립감사 07의 통과는 캐시 복구와 해당 개발 실행 파일의 격리 실행 범위에 한정되며, 전체 제품 완료·설치 가능 상태는 `BLOCKED`다. 실행 방법과 실제 수치는 시험 안내에서 구분한다. 기존 R28 실행 파일이나 앞선 후보의 결과를 이번 후보의 검증 근거로 사용하지 않는다.

- 실제로 로드된 미저장 로컬 대화는 `excludeTurns=true`이고 초기 이력 페이지를 요구하지 않을 때 현재 작성자 런타임과 세션 식별자·설정을 유지해 다른 연결에서 재개할 수 있다. 저장 이력이 필요한 요청, 로드되지 않은 대화와 임의 UUID는 이 예외에 포함하지 않는다. 저장소가 이력이 없다고 판정한 `None` 경로에서는 이미 저장된 작성자를 새 미저장 작성자용 대체 경로로 흡수하지 않는다. SQLite 메타데이터가 `Some`인 로드된 대화의 메타데이터 전용 재개는 파일 삭제 여부와 별개로 허용하던 기존 조회 계약을 유지한다.
- 소켓 경로를 생략한 기본 `app-server proxy`는 Windows에서 기존 비공개 연결 경로 검증과 비상승 권한 상대 검사를 수행한 뒤에만 데이터를 전달한다. 경로 검증 핸들은 접속·상대 인증이 끝날 때까지 유지한다. 명시적 소켓 경로와 Windows 이외 경로의 기존 동작은 유지한다.
- 작성한 회귀는 프록시 3개 함수의 3사례, 재개 4개 함수의 10사례로 총 13사례이며 표적 8개 그룹의 107개 통과 결과에 포함된다. 별도의 전체 TUI 시험은 5,849개 통과·0개 실패·4개 무시였다. 명령과 범위는 [시험 안내](docs/TESTING.md#공유-로컬-서버-후속-회귀시험검증-범위)에 적었다.

이전 후보 `eab9e295e54453bc30e00ae93c85d39ba738f596`의 독립감사 05 결과는 `FAIL`이었다. 당시 후보 `9766a8b7b8c4d56d7076367ddc12f2917a3df197`는 그 판정을 이어받지 않는다. 이전 갱신은 SQLite 설정별 회귀·MCP 시험 요청·판정 및 예약 도구 노출 기대값을 보강했고, 이번 갱신에는 미저장 대화의 커서 저장소 조건 수정도 포함된다.

MCP 회귀 요청에는 필수 `io.modelcontextprotocol/protocolVersion`·`io.modelcontextprotocol/clientCapabilities` 메타데이터를 넣어 전송 계층 거부를 예약 큐 검사로 오인하지 않도록 했다. 부정 응답은 정확한 오류 코드·메시지를 검사하고, 요청이 큐에 도착하기 전에 응답되면 본문을 표시하도록 시험을 보강했다. 해당 회귀를 포함한 실행 결과와 제한은 시험 안내에 기록했다.

이전 자연어 예약 후보 `921d404013da2035f90e9ab055acadc957a88592`의 전체 TUI 시험은 5,845개 통과·4개 실패·4개 건너뜀으로 종료 코드 101이었다. 실패 중 2개는 MCP 요청의 필수 메타데이터 누락으로 `a3edca405d1eddf138f549764459394603576804`에서 보강했다. 나머지 2개는 새 `cron` 노출을 빠뜨린 도구 목록·`deferLoading` 기대값으로 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609`에서 보강했다.

이번 시험 보강은 기존 작업트리 MCP에 `cron` 하나와 작업트리 도구 3개만 노출되는지, 원래 이름 공간과 입력 인자 제한을 유지하는지, 일반 작업 생성 호출은 정확한 오류로 거부되는지 검사한다. 외부 전달 방식은 `cron`만 `deferLoading=false`, 기존 비위임 도구 6개는 `true`로 남고 위임 도구는 포함하지 않도록 검사한다.

직전 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609` 후보의 표적 실행은 100개 통과·7개 실패였고, 8개 실행 그룹 중 3개 통과·5개 실패였다. 이전 `9766a8b7b8c4d56d7076367ddc12f2917a3df197` 후보에서는 표적 107개 통과·0개 실패와 전체 TUI 5,849개 통과·0개 실패·4개 무시를 확인했다. 이전 후보별 실패와 통과 기록은 새 후보의 진행 상황과 구분한다.

이번 제품 변경은 저장 메타데이터가 없는 미저장 대화의 페이징 재개에서 커서 저장소를 붙이지 않는 조건으로 한정한다. 나머지는 실제 원격 대화의 기록 준비와 같은 `sessionId` 확인, `deferLoading=false` 필드 생략·역직렬화 기본값 검사, 새 `cron` 도구가 포함된 작업트리 목록 스냅샷, 임시 접근 제어 목록 시험의 `Set-Acl`을 `icacls`로 바꾸는 보강이다. 접근 제어 목록 시험은 감사 권한을 요청하지 않는다. 빈 미저장 대화의 실제 화면 전환까지 해결됐다고 주장하지 않는다.

일반 개발 실행 파일의 제한된 격리 시험에서는 기본 프록시의 두 이력 형식 동일 세션 재개·프록시 종료 후 서버 유지와 용량 오류 세 조건을 확인했다. 이전 transport 메타데이터 재사용으로 실패한 같은 소스의 빌드는 관련 지문 6개만 보존 이동한 뒤 성공했다. 최초 유입 경위와 향후 재혼입 방지는 미확인이다. [실제 시험 수치·캐시 복구·감사 07 범위](docs/TESTING.md#공유-로컬-서버-후속-회귀시험검증-범위)를 참고한다.

관련 소스·시험 경로:

- `codex-rs/cli/src/main.rs`
- `codex-rs/stdio-to-uds/Cargo.toml`
- `codex-rs/stdio-to-uds/src/lib.rs`
- `codex-rs/stdio-to-uds/src/local_daemon_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/tests/suite/v2/thread_resume.rs`
- `codex-rs/tui/src/app/cron_jobs.rs`
- `codex-rs/tui/src/app/tests/active_reconnect_tests.rs`

- `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs`
- `codex-rs/tui/src/managed_worktree_tools_tests.rs`

- `codex-rs/uds/src/lib_tests.rs`
- `codex-rs/tui/src/snapshots/codex_tui__managed_worktree_tools__tests__model_tests__managed_worktree_model_creation.snap`

## 빈 미저장 대화 이력 복원 후속 후보 — 2026-10-30

이전 고정 소스 `67404f486047863ff7e56d7f47f4c350a872b483`는 `004c9113ff02c292998f1fa03355afe3dfe6d404` 이후 시험 3파일만 보강한 후보다. 제품 코드는 동일하다. 직전 `4f`의 화면·계약 시험은 통과했지만 전체 TUI 시험에서 1개가 실패했으며 시험 안내에 기록했다. 직전 `92d`의 06 TUI 컴파일은 시험 준비 호출의 `anyhow::Error`를 `eyre`로 자동 변환하지 못한 `E0277`로 종료 코드 101이었고 실행시험·일반 실행 파일 빌드는 미실행이다. `704`는 그 호출 한 곳을 인접 시험과 같은 명시적 준비 실패 `expect`로 바꿨다. `704`의 07 시험 컴파일은 통과했지만 대화 선택 시험은 1개 통과·2개 실패였고 일반 개발 빌드는 미실행이다. 이번 `674`는 설정 변경 알림과 `includeTurns=false` 생략 계약을 시험에 반영한 수정이다. 08 일반 개발 CLI·TUI 시험 컴파일과 영향 시험 66개, 고정 개발 실행 파일의 격리 시험 두 실행기는 종료 코드 0이었다. 전체 TUI를 새로 전량 통과한 결과는 아니다. 독립감사 08은 고정 소스·개발 실행 파일·66개 영향 시험·격리 실행에 한정해 PASS였다. 이후 최적화 CLI·별도 패키지 검사와 실제 Desktop 공유 실행도 확인했고 독립감사 09도 별도 패키지·최적화 격리 실행·공유 8검사 범위에서 `PASS(한정)`였다. 사용자 승인 후 새 명령줄 바로가기·기본 공유 서버를 전환했고 사후감사 11은 그 범위에서 `PASS(한정)`였다. 모바일·장시간 부하는 미완료다. 범위는 시험 안내에 구분했다. 이전 `9766` 후보의 표적 107개·전체 TUI 5,849개 통과와 고정 개발 실행 파일·감사 07 결과는 이번 후보의 통과 근거가 아니다.

서버는 같은 로컬 작성자가 실제로 로드돼 있고 영속 파일 생성을 미룬 상태인 초기 이력 조회의 `-32600` 오류에만 `kind=historyProjectionUnavailable`, `reason=unmaterialized` 표식을 더한다. 기존 코드·메시지는 유지하며 삭제된 저장 이력, 커서·고정점 요청과 다른 오류에 이 예외를 넓히지 않도록 작성했다.

TUI는 성공한 재개와 원래 세션·연결을 유지하면서 이력 미조회 상태를 보존하고 입력·실시간 이벤트를 계속 처리하도록 변경했다. 오류를 한 번 표시하거나 새 이벤트를 받았다는 이유로 이력을 조회 완료로 바꾸지 않는다. 빈 재생에도 종료 이벤트를 보내 현재 표시 대화의 이력 상태를 반영하고, 이전 커서가 없는 `Home` 동작이 로딩 상태에 고정되지 않도록 했다. 명시적인 이력 조회 성공 뒤에만 해당 미조회 상태를 해제하도록 작성했다.

실제 원격 요청·응답과 화면 이벤트를 통과하는 회귀, 오류 코드·표식의 부정 경계, 삭제된 저장 이력의 기존 계약 검사를 보강했다. 소스·시험 코드를 반영한 것이며 새 후보의 실행 결과나 전체 제품 완료를 뜻하지 않는다. [후속 시험 방법](docs/TESTING.md#빈-미저장-대화-이력-복원-후속-후보)을 참고한다.
