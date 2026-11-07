"""Export pinned public Git blobs, not working files, operational checks, or Git history."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile

OFFICIAL='086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a'
PATCHED='c4399dce957bb63fe1fdc276aa4306c06f6cf47f'
PREVIOUS_PUBLIC='e54da9ee059278403684061bb86e1e7072c16165'
PREFIXES=['codex-rs', 'third_party', 'patches', '.cargo/config.toml', 'tools/windows-cli', '.gitattributes', 'docs/native-cli-r27.ko.md', 'docs/native-cli-cron.ko.md', 'docs/native-cli-r33-cache-observation.ko.md', 'docs/native-cli-r35.ko.md', 'docs/native-cli-r36.ko.md', 'docs/r36-cleanup.ko.md', 'docs/r36-cloud-ci.ko.md', 'docs/r36-fast-compaction.ko.md', 'docs/r37-agent-selection-cases.ko.md', 'docs/r37-cold-cleanup-cases.ko.md', 'docs/r37-config-cases.ko.md', 'docs/r37-daemon-cases.ko.md', 'docs/r37-memory-cases.ko.md', 'docs/r37-memory-transport-cases.ko.md', 'docs/r37-plugin-cache-cases.ko.md', 'docs/r37-replay-compact-cases.ko.md', 'sdk/python/src/openai_codex/generated/v2_all.py', 'docs/native-cli-r38.ko.md', 'docs/r38-daemon-test-cases.ko.md', 'docs/r38-plugin-cache-test-cases.ko.md', 'docs/r38-stop-hook-test-cases.ko.md', 'scripts/install/normalize_stop_hook_config.py', 'scripts/install/test_normalize_stop_hook_config.py', 'docs/r38-agent-main-return-test-cases.ko.md']
PROTECTED=['LICENSE','NOTICE','README.md','README.ko.md']
DATE='2026-11-01'
PATCH_PATH='upstream/official-0.167.0.patch'
LABEL='R32'
NOTICE_OVERRIDES={path:('R33','2026-11-02') for path in (
    'codex-rs/tui/src/app/replay_filter.rs',
    'codex-rs/tui/src/app/thread_routing.rs',
    'codex-rs/tui/src/app/tests/buffered_replay.rs',
    'codex-rs/app-server-daemon/README.md',
    'codex-rs/app-server-daemon/src/backend/mod.rs',
    'codex-rs/app-server-daemon/src/backend/pid.rs',
    'codex-rs/app-server-daemon/src/backend/pid_tests.rs',
    'codex-rs/app-server-daemon/Cargo.toml',
    'codex-rs/app-server-daemon/src/backend/pid_start.rs',
    'codex-rs/app-server-daemon/src/backend/windows.rs',
    'codex-rs/app-server-daemon/src/backend/windows_spawn.rs',
    'codex-rs/app-server-daemon/src/backend/windows_tests.rs',
    'codex-rs/app-server-daemon/src/lib.rs',
    'codex-rs/app-server-daemon/src/prepare_install.rs',
    'codex-rs/app-server-daemon/src/prepare_install_tests.rs',
    'codex-rs/app-server-daemon/src/status.rs',
    'codex-rs/cli/src/daemon_telemetry.rs',
    'codex-rs/cli/src/main.rs',
    'codex-rs/codex-api/tests/cache_batch_boundary.rs',
    'codex-rs/diagnostics/src/cache_diagnostics.rs',
    'docs/native-cli-r33-cache-observation.ko.md',
    'codex-rs/Cargo.lock',
    'codex-rs/Cargo.toml',
    'codex-rs/app-server-daemon/src/client.rs',
    'codex-rs/app-server-transport/src/transport/mod.rs',
    'codex-rs/app-server-transport/src/transport/remote_control/client_tracker.rs',
    'codex-rs/app-server-transport/src/transport/unix_socket.rs',
    'codex-rs/app-server-transport/src/transport/unix_socket_tests.rs',
    'codex-rs/app-server/src/lib.rs',
    'codex-rs/app-server/src/bespoke_event_handling.rs',
    'codex-rs/app-server/src/outgoing_message.rs',
    'codex-rs/app-server/src/thread_status.rs',
    'codex-rs/app-server/src/lib_tests.rs',
    'codex-rs/cli/Cargo.toml',
    'codex-rs/cli/src/bin/opendiy_codex_launcher.rs',
    'codex-rs/cli/src/cleanup_agents.rs',
    'codex-rs/cli/src/lib.rs',
    'codex-rs/cli/src/opendiy_update.rs',
    'codex-rs/cli/src/opendiy_update_tests.rs',
    'codex-rs/cli/tests/update.rs',
    'codex-rs/tui/src/app_server_connection.rs',
    'codex-rs/tui/src/cleanup_agents_command.rs',
    'codex-rs/tui/src/daemon_startup.rs',
    'codex-rs/tui/src/daemon_startup_tests.rs',
    'codex-rs/tui/src/lib.rs',
    'codex-rs/tui/src/update_action.rs',
    'codex-rs/tui/src/updates.rs',
    'tools/windows-cli/README.md',
    'tools/windows-cli/UPDATE.ko.md',
    'tools/windows-cli/test_update.ps1',
    'tools/windows-cli/update.ps1',
    'tools/windows-cli/update.vbs',
    'tools/windows-cli/package_cli.py',
    'tools/windows-cli/test_package_cli.py',
)}
NOTICE_OVERRIDES.update({path: ('R33', '2026-11-03') for path in (
    'tools/windows-cli/update.ps1',
    'tools/windows-cli/test_update.ps1',
    'tools/windows-cli/test_update_output.py',
)})
NOTICE_OVERRIDES.update({path: ('R34', '2026-11-03') for path in (
    'codex-rs/Cargo.toml', 'codex-rs/Cargo.lock',
    'codex-rs/cli/Cargo.toml', 'codex-rs/cli/tests/daemon_startup.rs',
    'codex-rs/cli/src/main.rs', 'codex-rs/tui/src/main.rs',
    'codex-rs/tui/src/lib.rs', 'codex-rs/tui/src/lib_tests.rs',
    'codex-rs/tui/src/tui.rs', 'codex-rs/tui/src/tui_panic_tests.rs',
    'codex-rs/tui/src/app_server_session/history.rs',
    'codex-rs/tui/src/chatwidget.rs', 'codex-rs/tui/src/keymap.rs',
    'codex-rs/tui/src/app/tests/startup_defaults_tests.rs',
    'tools/windows-cli/update.ps1', 'tools/windows-cli/test_update.ps1',
    'tools/windows-cli/test_update_output.py',
)})
NOTICE_OVERRIDES.update({path: ('R35', '2026-11-03') for path in (
    'codex-rs/codex-api/src/sse/responses.rs',
    'codex-rs/codex-api/src/sse/cache_observation.rs',
    'codex-rs/Cargo.lock',
    'codex-rs/app-server-protocol/src/lib.rs',
    'codex-rs/app-server-protocol/src/protocol/common.rs',
    'codex-rs/app-server-protocol/src/protocol/v1.rs',
    'codex-rs/app-server-protocol/src/protocol/v2/tests.rs',
    'codex-rs/app-server-transport/src/transport/mod.rs',
    'codex-rs/app-server-transport/src/transport/remote_control/segment_tests.rs',
    'codex-rs/app-server-transport/src/transport/remote_control/tests.rs',
    'codex-rs/app-server/README.md',
    'codex-rs/app-server/src/request_processors/initialize_processor.rs',
    'codex-rs/app-server/tests/suite/v2/initialize.rs',
    'codex-rs/app-server/tests/suite/v2/thread_resume.rs',
    'codex-rs/cli/Cargo.toml',
    'codex-rs/cli/src/localization.rs',
    'codex-rs/cli/src/main.rs',
    'codex-rs/cli/tests/ui_language.rs',
    'codex-rs/cloud-tasks/src/app.rs',
    'codex-rs/cloud-tasks/src/cli.rs',
    'codex-rs/cloud-tasks/src/env_detect.rs',
    'codex-rs/cloud-tasks/src/env_detect_tests.rs',
    'codex-rs/cloud-tasks/src/lib.rs',
    'codex-rs/cloud-tasks/src/new_task.rs',
    'codex-rs/cloud-tasks/src/ui.rs',
    'codex-rs/cloud-tasks/src/util.rs',
    'codex-rs/config/Cargo.toml',
    'codex-rs/config/src/ui_language.rs',
    'codex-rs/tui/Cargo.toml',
    'codex-rs/tui/locales/en/cli.ftl',
    'codex-rs/tui/locales/en/cloud.ftl',
    'codex-rs/tui/locales/en/controls.ftl',
    'codex-rs/tui/locales/en/core.ftl',
    'codex-rs/tui/locales/en/dialogs.ftl',
    'codex-rs/tui/locales/en/legacy.ftl',
    'codex-rs/tui/locales/en/picker.ftl',
    'codex-rs/tui/locales/en/status.ftl',
    'codex-rs/tui/locales/ko/cli.ftl',
    'codex-rs/tui/locales/ko/cloud.ftl',
    'codex-rs/tui/locales/ko/controls.ftl',
    'codex-rs/tui/locales/ko/core.ftl',
    'codex-rs/tui/locales/ko/dialogs.ftl',
    'codex-rs/tui/locales/ko/legacy.ftl',
    'codex-rs/tui/locales/ko/picker.ftl',
    'codex-rs/tui/locales/ko/status.ftl',
    'codex-rs/tui/src/app/agent_center/hints.rs',
    'codex-rs/tui/src/app/agent_center/render.rs',
    'codex-rs/tui/src/app/agent_center/rows.rs',
    'codex-rs/tui/src/app/agents_overview.rs',
    'codex-rs/tui/src/app/agents_overview_details.rs',
    'codex-rs/tui/src/app/agents_overview_grouping.rs',
    'codex-rs/tui/src/app/agents_overview_view.rs',
    'codex-rs/tui/src/app/clipboard.rs',
    'codex-rs/tui/src/app/cron_jobs.rs',
    'codex-rs/tui/src/app/exit_summary.rs',
    'codex-rs/tui/src/app/input.rs',
    'codex-rs/tui/src/app/reconnect.rs',
    'codex-rs/tui/src/app/server_version_notice.rs',
    'codex-rs/tui/src/app/session_control.rs',
    'codex-rs/tui/src/app/session_picker.rs',
    'codex-rs/tui/src/app/startup.rs',
    'codex-rs/tui/src/app/transcript_export.rs',
    'codex-rs/tui/src/bottom_pane/action_required_title.rs',
    'codex-rs/tui/src/bottom_pane/actionable_banner.rs',
    'codex-rs/tui/src/bottom_pane/app_link_view.rs',
    'codex-rs/tui/src/bottom_pane/apply_patch_header.rs',
    'codex-rs/tui/src/bottom_pane/approval_overlay.rs',
    'codex-rs/tui/src/bottom_pane/async_questions/mod.rs',
    'codex-rs/tui/src/bottom_pane/async_questions/render.rs',
    'codex-rs/tui/src/bottom_pane/async_questions/state.rs',
    'codex-rs/tui/src/bottom_pane/async_questions/state_tests.rs',
    'codex-rs/tui/src/bottom_pane/chat_composer.rs',
    'codex-rs/tui/src/bottom_pane/chat_composer/footer_state.rs',
    'codex-rs/tui/src/bottom_pane/chat_composer/history_search.rs',
    'codex-rs/tui/src/bottom_pane/chat_composer/warning_notice.rs',
    'codex-rs/tui/src/bottom_pane/command_popup.rs',
    'codex-rs/tui/src/bottom_pane/custom_prompt_view/picker.rs',
    'codex-rs/tui/src/bottom_pane/experimental_features_view.rs',
    'codex-rs/tui/src/bottom_pane/feedback_note_view.rs',
    'codex-rs/tui/src/bottom_pane/feedback_note_view_tests.rs',
    'codex-rs/tui/src/bottom_pane/feedback_view.rs',
    'codex-rs/tui/src/bottom_pane/file_search_popup.rs',
    'codex-rs/tui/src/bottom_pane/footer.rs',
    'codex-rs/tui/src/bottom_pane/hooks_browser_render.rs',
    'codex-rs/tui/src/bottom_pane/hooks_browser_render_tests.rs',
    'codex-rs/tui/src/bottom_pane/hooks_browser_view.rs',
    'codex-rs/tui/src/bottom_pane/list_selection_view.rs',
    'codex-rs/tui/src/bottom_pane/mcp_server_elicitation.rs',
    'codex-rs/tui/src/bottom_pane/memories_settings_view.rs',
    'codex-rs/tui/src/bottom_pane/memories_settings_view_tests.rs',
    'codex-rs/tui/src/bottom_pane/mentions_v2/candidate.rs',
    'codex-rs/tui/src/bottom_pane/mentions_v2/footer.rs',
    'codex-rs/tui/src/bottom_pane/mentions_v2/popup.rs',
    'codex-rs/tui/src/bottom_pane/mentions_v2/render.rs',
    'codex-rs/tui/src/bottom_pane/mentions_v2/render_tests.rs',
    'codex-rs/tui/src/bottom_pane/mentions_v2/search_catalog.rs',
    'codex-rs/tui/src/bottom_pane/mentions_v2/search_mode.rs',
    'codex-rs/tui/src/bottom_pane/multi_select_picker.rs',
    'codex-rs/tui/src/bottom_pane/pending_input_preview.rs',
    'codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs',
    'codex-rs/tui/src/bottom_pane/popup_consts.rs',
    'codex-rs/tui/src/bottom_pane/questions.rs',
    'codex-rs/tui/src/bottom_pane/request_user_input/mod.rs',
    'codex-rs/tui/src/bottom_pane/request_user_input/render.rs',
    'codex-rs/tui/src/bottom_pane/selection_popup_common.rs',
    'codex-rs/tui/src/bottom_pane/selection_row_layout.rs',
    'codex-rs/tui/src/bottom_pane/shortcut_hud.rs',
    'codex-rs/tui/src/bottom_pane/shortcut_overlay.rs',
    'codex-rs/tui/src/bottom_pane/shortcut_overlay_tests.rs',
    'codex-rs/tui/src/bottom_pane/skill_popup.rs',
    'codex-rs/tui/src/bottom_pane/skills_toggle_view.rs',
    'codex-rs/tui/src/bottom_pane/textarea.rs',
    'codex-rs/tui/src/bottom_pane/title_setup.rs',
    'codex-rs/tui/src/bottom_pane/unified_exec_footer.rs',
    'codex-rs/tui/src/bottom_pane/user_verification.rs',
    'codex-rs/tui/src/bottom_pane/voice_strip.rs',
    'codex-rs/tui/src/bottom_pane/voice_strip_tests.rs',
    'codex-rs/tui/src/bottom_pane/warnings_view_render.rs',
    'codex-rs/tui/src/chatwidget/clipboard.rs',
    'codex-rs/tui/src/chatwidget/goal_menu.rs',
    'codex-rs/tui/src/chatwidget/ide_context.rs',
    'codex-rs/tui/src/chatwidget/interaction.rs',
    'codex-rs/tui/src/chatwidget/model_popups.rs',
    'codex-rs/tui/src/chatwidget/realtime_settings.rs',
    'codex-rs/tui/src/clipboard_copy.rs',
    'codex-rs/tui/src/history_cell/cron.rs',
    'codex-rs/tui/src/history_cell/mod.rs',
    'codex-rs/tui/src/i18n.rs',
    'codex-rs/tui/src/i18n/catalog.rs',
    'codex-rs/tui/src/ide_context/ipc.rs',
    'codex-rs/tui/src/onboarding/bedrock.rs',
    'codex-rs/tui/src/onboarding/bedrock_tests.rs',
    'codex-rs/tui/src/onboarding/onboarding_screen.rs',
    'codex-rs/tui/src/resume_picker.rs',
    'codex-rs/tui/src/resume_picker/archive.rs',
    'codex-rs/tui/src/resume_picker/sources.rs',
    'codex-rs/tui/src/slash_command.rs',
    'codex-rs/tui/src/status/card.rs',
    'codex-rs/tui/src/status/format.rs',
    'codex-rs/tui/src/status/helpers.rs',
    'codex-rs/tui/src/status/rate_limits.rs',
    'codex-rs/tui/src/status/remote_connection.rs',
    'codex-rs/tui/src/status/tests.rs',
    'codex-rs/tui/src/status/thread_usage.rs',
    'codex-rs/tui/src/status/thread_usage_tests.rs',
    'codex-rs/tui/src/token_usage.rs',
    'codex-rs/tui/src/transcript_view/input_tests.rs',
)})
NOTICE_OVERRIDES.update({path: ('R35', '2026-11-03') for path in (
    "codex-rs/Cargo.lock",
    "codex-rs/Cargo.toml",
    "codex-rs/app-server-daemon/src/lib.rs",
    "codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json",
    "codex-rs/app-server-protocol/schema/json/v1/InitializeResponse.json",
    "codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst",
    "codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst",
    "codex-rs/app-server-protocol/schema/typescript/InitializeResponse.ts",
    "codex-rs/app-server-protocol/schema/typescript/OpenDiyCapabilities.ts",
    "codex-rs/app-server-protocol/schema/typescript/index.ts",
    "codex-rs/cli/src/localization.rs",
    "codex-rs/cli/src/main.rs",
    "codex-rs/core/src/config/runtime_refresh.rs",
    "codex-rs/core/src/config/runtime_refresh_tests.rs",
    "codex-rs/diagnostics/Cargo.toml",
    "codex-rs/diagnostics/src/batch_log.rs",
    "codex-rs/diagnostics/src/cache_diagnostics.rs",
    "codex-rs/diagnostics/src/lib.rs",
    "codex-rs/diagnostics/src/settings_control.rs",
    "codex-rs/diagnostics/src/settings_control_tests.rs",
    "codex-rs/tui/locales/en/cli.ftl",
    "codex-rs/tui/locales/en/controls.ftl",
    "codex-rs/tui/locales/en/legacy.ftl",
    "codex-rs/tui/locales/ko/cli.ftl",
    "codex-rs/tui/locales/ko/controls.ftl",
    "codex-rs/tui/locales/ko/legacy.ftl",
    "codex-rs/tui/src/app.rs",
    "codex-rs/tui/src/app/diagnostics_control.rs",
    "codex-rs/tui/src/app/diagnostics_control_tests.rs",
    "codex-rs/tui/src/app/memory.rs",
    "codex-rs/tui/src/app/session_control.rs",
    "codex-rs/tui/src/app/session_control_tests.rs",
    "codex-rs/tui/src/app_server_connection.rs",
    "codex-rs/tui/src/app_server_session.rs",
    "codex-rs/tui/src/bottom_pane/async_questions/render.rs",
    "codex-rs/tui/src/bottom_pane/async_questions/state_tests.rs",
    "codex-rs/tui/src/bottom_pane/warnings_view_render.rs",
    "codex-rs/tui/src/bottom_pane/warnings_view_tests.rs",
    "codex-rs/tui/src/chatwidget/interaction.rs",
    "codex-rs/tui/src/chatwidget/interaction_tests.rs",
    "codex-rs/tui/src/cleanup_agents_command.rs",
    "codex-rs/tui/src/cleanup_agents_command_tests.rs",
    "codex-rs/tui/src/daemon_startup_tests.rs",
    "codex-rs/tui/src/keymap.rs",
    "codex-rs/tui/src/session_control.rs",
    "docs/native-cli-r35.ko.md",
)})
ATTR_BEGIN=b'# BEGIN OpenDIY pinned source attributes\n'
ATTR_END=b'# END OpenDIY pinned source attributes\n'
NOTICE_OVERRIDES.update({path: ('R36', '2026-11-03') for path in (
    'codex-rs/Cargo.lock',
    'codex-rs/Cargo.toml',
    'codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json',
    'codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json',
    'codex-rs/app-server-protocol/schema/json/v2/ThreadCleanupCompletedResponse.json',
    'codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst',
    'codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst',
    'codex-rs/app-server-protocol/schema/typescript/v2/CompletedChildCleanupResult.ts',
    'codex-rs/app-server-protocol/src/protocol/v2/thread.rs',
    'codex-rs/app-server/src/request_processors/thread_delete.rs',
    'codex-rs/cli/src/ci.rs',
    'codex-rs/cli/src/cleanup_agents.rs',
    'codex-rs/cli/src/localization.rs',
    'codex-rs/cli/src/main.rs',
    'codex-rs/core/Cargo.toml',
    'codex-rs/core/src/agent/control/mailbox.rs',
    'codex-rs/core/src/agent/control/runtime.rs',
    'codex-rs/core/src/compact.rs',
    'codex-rs/core/src/compact_remote_v2.rs',
    'codex-rs/core/src/compact_remote_v2_attempt.rs',
    'codex-rs/core/src/compact_remote_v2_installation.rs',
    'codex-rs/core/src/compact_tests.rs',
    'codex-rs/core/src/tasks/compact.rs',
    'codex-rs/core/src/tasks/mod.rs',
    'codex-rs/core/src/thread_manager/cleanup.rs',
    'codex-rs/core/src/thread_manager/cleanup_tests.rs',
    'codex-rs/core/src/thread_manager/cleanup_unloaded_tests.rs',
    'codex-rs/core/tests/subagent_service_tier.rs',
    'codex-rs/core/tests/suite/mod.rs',
    'codex-rs/core/tests/suite/subagent_service_tier.rs',
    'codex-rs/core/tests/suite/test_binary_dispatch.rs',
    'codex-rs/rollout/src/recorder.rs',
    'codex-rs/rollout/src/recorder_tests.rs',
    'codex-rs/rollout/src/writer_lock.rs',
    'codex-rs/rollout/src/writer_lock_tests.rs',
    'codex-rs/thread-store/src/lib.rs',
    'codex-rs/thread-store/src/local/archive_thread.rs',
    'codex-rs/thread-store/src/local/delete_thread.rs',
    'codex-rs/thread-store/src/local/live_writer.rs',
    'codex-rs/thread-store/src/local/mod.rs',
    'codex-rs/thread-store/src/local/paginated_fork.rs',
    'codex-rs/thread-store/src/local/revert_thread.rs',
    'codex-rs/thread-store/src/local/revert_thread_tests.rs',
    'codex-rs/thread-store/src/local/thread_attachments_tests.rs',
    'codex-rs/thread-store/src/local/thread_history_materialization_tests.rs',
    'codex-rs/thread-store/src/local/unarchive_thread.rs',
    'codex-rs/thread-store/src/local/unloaded_cleanup.rs',
    'codex-rs/tui/locales/en/cli.ftl',
    'codex-rs/tui/locales/en/r36-agents.ftl',
    'codex-rs/tui/locales/en/r36-ci.ftl',
    'codex-rs/tui/locales/en/r36-cleanup.ftl',
    'codex-rs/tui/locales/ko/cli.ftl',
    'codex-rs/tui/locales/ko/r36-agents.ftl',
    'codex-rs/tui/locales/ko/r36-ci.ftl',
    'codex-rs/tui/locales/ko/r36-cleanup.ftl',
    'codex-rs/tui/src/app/agent_navigation.rs',
    'codex-rs/tui/src/app/agent_status_feed.rs',
    'codex-rs/tui/src/app/subagent_status_tests.rs',
    'codex-rs/tui/src/app/thread_routing.rs',
    'codex-rs/tui/src/bottom_pane/mod.rs',
    'codex-rs/tui/src/bottom_pane/subagent_status.rs',
    'codex-rs/tui/src/cleanup_agents_command.rs',
    'codex-rs/tui/src/cleanup_agents_command_tests.rs',
    'codex-rs/tui/src/i18n/catalog.rs',
    'docs/native-cli-r36.ko.md',
    'docs/r36-cleanup.ko.md',
    'docs/r36-cloud-ci.ko.md',
    'docs/r36-fast-compaction.ko.md',
)})
# Only paths changed by the pinned R37 commit receive the R37 notice.
NOTICE_OVERRIDES.update({path: ('R37', '2026-11-04') for path in (
    'codex-rs/Cargo.lock',
    'codex-rs/Cargo.toml',
    'codex-rs/app-server-client/src/lib.rs',
    'codex-rs/app-server-client/src/remote.rs',
    'codex-rs/app-server-daemon/src/launch.rs',
    'codex-rs/app-server-daemon/src/launch_tests.rs',
    'codex-rs/app-server-daemon/src/lib.rs',
    'codex-rs/app-server-daemon/src/prepare_install.rs',
    'codex-rs/app-server-protocol/schema/json/ClientRequest.json',
    'codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json',
    'codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json',
    'codex-rs/app-server-protocol/schema/json/v1/InitializeResponse.json',
    'codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst',
    'codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst',
    'codex-rs/app-server-protocol/schema/typescript/OpenDiyCapabilities.ts',
    'codex-rs/app-server-protocol/schema/typescript/v2/MemoryConsolidationState.ts',
    'codex-rs/app-server-protocol/schema/typescript/v2/MemoryNoteScope.ts',
    'codex-rs/app-server-protocol/schema/typescript/v2/index.ts',
    'codex-rs/app-server-protocol/src/protocol/common.rs',
    'codex-rs/app-server-protocol/src/protocol/v1.rs',
    'codex-rs/app-server-protocol/src/protocol/v2/memory.rs',
    'codex-rs/app-server-protocol/src/protocol/v2/tests.rs',
    'codex-rs/app-server/src/in_process.rs',
    'codex-rs/app-server/src/message_processor.rs',
    'codex-rs/app-server/src/request_processors/initialize_processor.rs',
    'codex-rs/app-server/src/request_processors/thread_memory.rs',
    'codex-rs/app-server/tests/suite/v2/external_agent_memory_concurrency.rs',
    'codex-rs/app-server/tests/suite/v2/initialize.rs',
    'codex-rs/app-server/tests/suite/v2/memory_consolidate.rs',
    'codex-rs/app-server/tests/suite/v2/memory_read.rs',
    'codex-rs/cli/src/remote_control_cmd.rs',
    'codex-rs/cli/tests/ui_language.rs',
    'codex-rs/config/src/ui_language.rs',
    'codex-rs/core-plugins/src/store.rs',
    'codex-rs/core-plugins/src/store_tests.rs',
    'codex-rs/diagnostics/src/batch_log.rs',
    'codex-rs/memories/write/src/explicit_consolidation_tests.rs',
    'codex-rs/memories/write/src/lib.rs',
    'codex-rs/memories/write/src/phase2.rs',
    'codex-rs/memories/write/src/start.rs',
    'codex-rs/memories/write/src/startup_tests.rs',
    'codex-rs/state/src/lib.rs',
    'codex-rs/state/src/runtime.rs',
    'codex-rs/state/src/runtime/memories.rs',
    'codex-rs/state/src/runtime/memory_consolidation_tests.rs',
    'codex-rs/tui/locales/en/controls.ftl',
    'codex-rs/tui/locales/en/r36-agents.ftl',
    'codex-rs/tui/locales/ko/controls.ftl',
    'codex-rs/tui/locales/ko/r36-agents.ftl',
    'codex-rs/tui/src/app.rs',
    'codex-rs/tui/src/app/agent_navigation.rs',
    'codex-rs/tui/src/app/event_dispatch.rs',
    'codex-rs/tui/src/app/input.rs',
    'codex-rs/tui/src/app/owned_transcript.rs',
    'codex-rs/tui/src/app/replay_filter.rs',
    'codex-rs/tui/src/app/session_control.rs',
    'codex-rs/tui/src/app/session_control_tests.rs',
    'codex-rs/tui/src/app/subagent_status_tests.rs',
    'codex-rs/tui/src/app/tests/active_reconnect_tests.rs',
    'codex-rs/tui/src/app/tests/buffered_replay.rs',
    'codex-rs/tui/src/app/thread_routing.rs',
    'codex-rs/tui/src/app_event.rs',
    'codex-rs/tui/src/app_server_session.rs',
    'codex-rs/tui/src/bottom_pane/mod.rs',
    'codex-rs/tui/src/bottom_pane/subagent_status.rs',
    'codex-rs/tui/src/chatwidget/interaction.rs',
    'codex-rs/tui/src/chatwidget/questions.rs',
    'codex-rs/tui/src/i18n.rs',
    'codex-rs/tui/src/resume_picker.rs',
    'codex-rs/tui/src/session_control.rs',
    'codex-rs/tui/src/startup_orchestration.rs',
    'codex-rs/tui/src/startup_orchestration_tests.rs',
    'codex-rs/tui/src/tui.rs',
    'codex-rs/tui/src/tui/alternate_screen.rs',
    'codex-rs/tui/src/tui/alternate_screen_tests.rs',
    'codex-rs/tui/src/tui/alternate_screen_windows_tests.rs',
    'codex-rs/tui/src/tui/job_control.rs',
    'codex-rs/tui/src/tui/owned_screen_tests.rs',
    'docs/r37-agent-selection-cases.ko.md',
    'docs/r37-cold-cleanup-cases.ko.md',
    'docs/r37-config-cases.ko.md',
    'docs/r37-daemon-cases.ko.md',
    'docs/r37-memory-cases.ko.md',
    'docs/r37-memory-transport-cases.ko.md',
    'docs/r37-plugin-cache-cases.ko.md',
    'docs/r37-replay-compact-cases.ko.md',
    'sdk/python/src/openai_codex/generated/v2_all.py',
)})
WINDOWS_INPUTS_PATH='upstream/windows-prompt-inputs.json'
WINDOWS_INPUTS={p['path']:p for p in json.loads((Path(__file__).resolve().parents[2]/WINDOWS_INPUTS_PATH).read_bytes())['files']}

NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in ('codex-rs/Cargo.lock', 'codex-rs/Cargo.toml', 'codex-rs/core-plugins/src/store.rs', 'codex-rs/core-plugins/src/store_tests.rs', 'codex-rs/hooks/src/engine/discovery.rs', 'codex-rs/tui/locales/en/cli.ftl', 'codex-rs/tui/locales/en/controls.ftl', 'codex-rs/tui/locales/en/r36-agents.ftl', 'codex-rs/tui/locales/ko/cli.ftl', 'codex-rs/tui/locales/ko/controls.ftl', 'codex-rs/tui/locales/ko/r36-agents.ftl', 'codex-rs/tui/src/app/agent_navigation.rs', 'codex-rs/tui/src/app/reconnect.rs', 'codex-rs/tui/src/app/session_control.rs', 'codex-rs/tui/src/app/startup.rs', 'codex-rs/tui/src/app/subagent_status_tests.rs', 'codex-rs/tui/src/app/tests/backend_ownership_tests.rs', 'codex-rs/tui/src/app/tests/disconnect_tests.rs', 'codex-rs/tui/src/app/tests/plugin_reload_tests.rs', 'codex-rs/tui/src/app/thread_routing.rs', 'codex-rs/tui/src/app_server_connection.rs', 'codex-rs/tui/src/bottom_pane/subagent_status.rs', 'codex-rs/tui/src/daemon_startup.rs', 'codex-rs/tui/src/daemon_startup_tests.rs', 'codex-rs/tui/src/history_cell/notices.rs', 'codex-rs/tui/src/history_cell/tests.rs', 'codex-rs/tui/src/i18n.rs', 'codex-rs/tui/src/lib.rs', 'codex-rs/tui/src/session_archive_commands.rs', 'codex-rs/tui/src/startup_orchestration.rs', 'codex-rs/tui/src/startup_orchestration_tests.rs', 'docs/native-cli-r38.ko.md', 'docs/r38-agent-main-return-test-cases.ko.md', 'docs/r38-daemon-test-cases.ko.md', 'docs/r38-plugin-cache-test-cases.ko.md', 'docs/r38-stop-hook-test-cases.ko.md', 'scripts/install/normalize_stop_hook_config.py', 'scripts/install/test_normalize_stop_hook_config.py')})
NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in ('codex-rs/core/src/session/mod.rs', 'codex-rs/core/src/session/tests.rs')})

# R38 0.167 integration: only directly edited, changed selected source files.
NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in (
    'codex-rs/Cargo.lock',
    'codex-rs/Cargo.toml',
    'codex-rs/app-server/README.md',
    'codex-rs/app-server/src/config_manager_service.rs',
    'codex-rs/app-server/src/outgoing_message.rs',
    'codex-rs/app-server/src/request_processors/thread_lifecycle.rs',
    'codex-rs/app-server/src/request_processors/turn_processor.rs',
    'codex-rs/cli/src/main.rs',
    'codex-rs/cli/tests/daemon_startup.rs',
    'codex-rs/codex-api/src/endpoint/responses_websocket.rs',
    'codex-rs/codex-mcp/src/connection_manager/test_support.rs',
    'codex-rs/codex-mcp/src/connection_manager_tests.rs',
    'codex-rs/codex-mcp/src/connection_pool.rs',
    'codex-rs/core/src/agent/control/spawn.rs',
    'codex-rs/core/src/agent/control/spawn_guard.rs',
    'codex-rs/core/src/session/handlers.rs',
    'codex-rs/core/src/session/turn.rs',
    'codex-rs/core/src/thread_manager.rs',
    'codex-rs/tui/src/app/agent_center/rows.rs',
    'codex-rs/tui/src/app/agent_navigation.rs',
    'codex-rs/tui/src/app/app_server_events.rs',
    'codex-rs/tui/src/app/backend_events.rs',
    'codex-rs/tui/src/app/backend_sessions.rs',
    'codex-rs/tui/src/app/cron_jobs.rs',
    'codex-rs/tui/src/app/exit_summary.rs',
    'codex-rs/tui/src/app/input.rs',
    'codex-rs/tui/src/app/session_picker.rs',
    'codex-rs/tui/src/app/startup.rs',
    'codex-rs/tui/src/app/tests/active_reconnect_tests.rs',
    'codex-rs/tui/src/app/thread_events.rs',
    'codex-rs/tui/src/app/thread_routing.rs',
    'codex-rs/tui/src/app_server_session.rs',
    'codex-rs/tui/src/app_server_session/rollout_history.rs',
    'codex-rs/tui/src/bottom_pane/pending_thread_approvals.rs',
    'codex-rs/tui/src/chatwidget/input_restore.rs',
    'codex-rs/tui/src/chatwidget/interaction.rs',
    'codex-rs/tui/src/chatwidget/provider_error.rs',
    'codex-rs/tui/src/chatwidget/rendering.rs',
    'codex-rs/tui/src/chatwidget/tests/composer_submission.rs',
    'codex-rs/tui/src/dynamic_tools_mcp.rs',
    'codex-rs/tui/src/i18n.rs',
    'codex-rs/tui/src/i18n/catalog.rs',
    'codex-rs/tui/src/lib.rs',
    'codex-rs/tui/src/managed_worktree_tools_tests.rs',
    'codex-rs/tui/src/onboarding/auth.rs',
    'codex-rs/tui/src/onboarding/bedrock.rs',
    'codex-rs/tui/src/onboarding/bedrock_tests.rs',
    'codex-rs/tui/src/onboarding/gcp.rs',
    'codex-rs/tui/src/resume_picker.rs',
    'codex-rs/tui/src/resume_picker/sources.rs',
    'codex-rs/tui/src/startup_orchestration.rs',
    'docs/native-cli-r38.ko.md',
)})

NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in (
    'codex-rs/tui/src/app/tests/session_lifecycle_requests.rs',
)})

NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in (
    'codex-rs/codex-mcp/src/rmcp_client.rs',
)})

NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in (
    'codex-rs/core/src/agent/control/legacy.rs',
    'codex-rs/core/src/config/edit_tests.rs',
    'codex-rs/core/src/session/config_refresh_tests.rs',
)})

NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in (
    'codex-rs/tui/src/app/tests/new_session_tests.rs',
    'codex-rs/tui/src/managed_worktree_model_tests.rs',
    'codex-rs/tui/src/onboarding/gcp_tests.rs',
)})

NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in (
    'codex-rs/tui/Cargo.toml',
)})

NOTICE_OVERRIDES.update({path: ('R38', '2026-11-06') for path in (
    'codex-rs/codex-mcp/src/connection_pool_tests.rs',
)})

# R39 compact-only followup: snapshots and runtime prompts stay exact bytes.
NOTICE_OVERRIDES.update({path: ('R39', '2026-11-07') for path in (
    'codex-rs/Cargo.lock',
    'codex-rs/Cargo.toml',
    'codex-rs/core/src/compact.rs',
    'codex-rs/core/src/compact_remote_v2.rs',
    'codex-rs/core/src/compact_remote_v2_attempt.rs',
    'codex-rs/core/tests/common/context_snapshot/normalize.rs',
    'codex-rs/core/tests/suite/compact.rs',
    'codex-rs/core/tests/suite/compact_remote.rs',
    'codex-rs/core/tests/suite/compact_resume_fork.rs',
    'codex-rs/core/tests/suite/direct_tool_metadata.rs',
    'codex-rs/core/tests/suite/guardian_history.rs',
    'codex-rs/core/tests/suite/guardian_retained_context.rs',
    'codex-rs/core/tests/suite/pending_input.rs',
    'codex-rs/core/tests/suite/scenarios.rs',
)})

def sha(data):return hashlib.sha256(data).hexdigest()
def git(repository,*args):return subprocess.check_output(['git','-c','core.autocrlf=false','-C',str(repository),*args])
def encode(value):return (json.dumps(value,ensure_ascii=False,indent=2)+'\n').encode()

def notice(path):
    label,date=NOTICE_OVERRIDES.get(path,(LABEL,DATE))
    if path.startswith('codex-rs/app-server-protocol/schema/typescript/') and path.endswith('.ts'):return b''
    if path.endswith(('.rs','.ts')):prefix='//'
    elif path.endswith(('.py','.toml','Cargo.lock','.gitattributes','.gitignore')):prefix='#'
    elif path.endswith('.ps1') and path in NOTICE_OVERRIDES:prefix='#'
    elif path.endswith('.vbs') and path in NOTICE_OVERRIDES:prefix="'"
    elif path.endswith('.md') and (path.startswith('docs/') or path in NOTICE_OVERRIDES):return f'<!-- Modified for OpenDIY Codex {label} ({date}); see MODIFICATIONS.md. -->\n'.encode()
    else:return b''  # Data formats and snapshots stay exact; MODIFICATIONS.md lists each one.
    return f'{prefix} Modified for OpenDIY Codex {label} ({date}); see MODIFICATIONS.md.\n'.encode()

def checked_target(root,path):
    relative=Path(path)
    target=root/relative
    if relative.is_absolute() or '..' in relative.parts or any(p in relative.parts for p in ['.git','target','node_modules','checks']) or not target.resolve().is_relative_to(root.resolve()):
        raise ValueError(f'Unsafe export target: {path}')
    if target.parent.resolve()!=root.resolve().joinpath(*relative.parts[:-1]):raise ValueError(f'Unexpected export parent symlink: {path}')
    if target.is_symlink() and (relative.as_posix()!='codex-rs/vendor/bubblewrap/LICENSE' or os.readlink(target)!='COPYING'):
        raise ValueError(f'Unexpected export symlink: {path}')
    return target

def canonical_bytes(relative,data):return data.replace(b'\r\n',b'\n') if relative in WINDOWS_INPUTS else data

def existing_bytes(path,relative):return canonical_bytes(relative,os.readlink(path).encode() if path.is_symlink() else path.read_bytes())

def prompt_attributes():
    return b'# Embedded prompt inputs use native checkout EOL; do not force Linux CRLF.\n'+''.join(f'{path} text\n' for path in WINDOWS_INPUTS).encode()

def publish(root,outputs,previous):
    # Check the whole update before changing anything. Only previously inventoried bytes may be replaced.
    for path,expected in previous.items():
        target=checked_target(root,path)
        if not target.is_file() or sha(existing_bytes(target,path))!=expected:raise ValueError(f'Previous import changed: {path}')
    for path,data in outputs.items():
        target=checked_target(root,path)
        if target.exists() and existing_bytes(target,path)!=data and path not in previous:raise ValueError(f'Existing file differs; refusing overwrite: {path}')
    for path,data in outputs.items():
        target=checked_target(root,path);target.parent.mkdir(parents=True,exist_ok=True)
        if not target.exists() or existing_bytes(target,path)!=data:target.write_bytes(data)
    for path in previous.keys()-outputs.keys():checked_target(root,path).unlink()

def pinned_previous_source(root,previous):
    # Later public test commits must be preserved in R31, never silently stamped.
    # Both their bytes and the import metadata are anchored to the reviewed Git commit.
    git(root,'diff','--quiet',PREVIOUS_PUBLIC,'--',*PREFIXES,'SOURCE_BASELINE.json','upstream/source-inventory.json')
    tracked=set(git(root,'ls-tree','-r','--name-only',PREVIOUS_PUBLIC,'--',*PREFIXES).decode().splitlines())
    tracked = {path for path in tracked if path in previous or not path.startswith('patches/')}
    previous=dict(previous)
    for path in tracked:
        target=checked_target(root,path)
        current=existing_bytes(target,path)
        if path not in previous or sha(current)!=previous[path]:
            pinned=canonical_bytes(path,git(root,'show',PREVIOUS_PUBLIC+':'+path))
            if current!=pinned:raise ValueError(f'Public source differs from reviewed commit: {path}')
            previous[path]=sha(pinned)
    return previous

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-repository',type=Path)
    parser.add_argument('--output',type=Path,default=Path(__file__).resolve().parents[2])
    parser.add_argument('--update',action='store_true',help='Replace only a verified previous source inventory; preserve repository documents')
    parser.add_argument('--stage-modes',action='store_true',help='Restore imported Git modes after git add; does not change file bytes')
    args=parser.parse_args();root=args.output.resolve();source=args.source_repository
    if args.stage_modes:
        inventory=json.loads((root/'upstream/source-inventory.json').read_bytes());staged={}
        for row in git(root,'ls-files','--stage','-z').split(b'\0'):
            if row:
                metadata,path=row.split(b'\t');staged[path.decode()]=metadata.decode().split()[1]
        rows=[]
        for item in inventory['files']:
            path=item['path'];data=existing_bytes(checked_target(root,path),path)
            if sha(data)!=item['publishedSha256'] or path not in staged:raise ValueError(f'Source not staged as reviewed: {path}')
            rows.append(f'{item["gitMode"]} {staged[path]}\t{path}\n')
        subprocess.run(['git','-C',str(root),'update-index','--index-info'],input=''.join(rows).encode(),check=True)
        print(json.dumps({'status':'PASS','stagedSourceModes':len(rows)}));return
    if source is None:parser.error('--source-repository is required for source export')
    if git(source,'rev-parse',OFFICIAL).decode().strip()!=OFFICIAL or git(source,'rev-parse',PATCHED).decode().strip()!=PATCHED:raise ValueError('Missing source commits')
    existing={p:sha((root/p).read_bytes()) for p in PROTECTED}
    previous={}
    if args.update:
        baseline=json.loads((root/'SOURCE_BASELINE.json').read_bytes())
        inventory_raw=(root/'upstream/source-inventory.json').read_bytes()
        if sha(inventory_raw)!=baseline['sourceInventorySha256']:raise ValueError('Previous inventory changed')
        previous={item['path']:item['publishedSha256'] for item in json.loads(inventory_raw)['files']}
        previous=pinned_previous_source(root,previous)
        if baseline.get('patchPath')==PATCH_PATH:previous[PATCH_PATH]=baseline['patchSha256']
        for p in ['SOURCE_BASELINE.json','upstream/source-inventory.json','upstream/LICENSE','upstream/NOTICE']:
            previous[p]=sha((root/p).read_bytes())
    attributes=(root/'.gitattributes').read_bytes() if (root/'.gitattributes').exists() else b''
    if (root/'.gitattributes').exists() and '.gitattributes' not in previous:previous['.gitattributes']=sha(attributes)
    if attributes.count(ATTR_BEGIN)!=attributes.count(ATTR_END) or attributes.count(ATTR_BEGIN)>1:raise ValueError('Unexpected managed attribute blocks')
    if ATTR_BEGIN in attributes:
        before,managed=attributes.split(ATTR_BEGIN,1)
        _,after=managed.split(ATTR_END,1);attributes=before+after
    changed=set(git(source,'diff','--name-only',OFFICIAL,PATCHED,'--',*PREFIXES).decode().splitlines())
    patch=git(source,'diff','--binary','--no-ext-diff','--no-textconv',OFFICIAL,PATCHED,'--',*PREFIXES)
    tree={}
    for row in git(source,'ls-tree','-r','-z',PATCHED,*PREFIXES).split(b'\0'):
        if row:
            metadata,path=row.split(b'\t');tree[path.decode()]=metadata.decode().split()[2]
    archive=git(source,'archive','--format=tar',PATCHED,*PREFIXES)
    inventory=[];excluded=[];symlinks=[];outputs={}
    with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
        for member in stream:
            path=member.name
            if member.isdir():continue
            if '/.github/' in path or path.startswith('.github/'):
                excluded.append({'path':path,'reason':'upstream automation is not enabled by this source import'});continue
            if any(p in path.split('/') for p in ['.git','target','node_modules','checks']) or path.lower().endswith(('.exe','.dll','.asar','.pdb','.sqlite','.sqlite3','.db','.log')):raise ValueError(f'Excluded binary/runtime path in public source selection: {path}')
            if member.issym():
                if path!='codex-rs/vendor/bubblewrap/LICENSE' or member.linkname!='COPYING':raise ValueError('Unexpected source symlink')
                raw=member.linkname.encode();symlinks.append({'path':path,'target':member.linkname})
            elif member.isfile():raw=stream.extractfile(member).read()
            else:raise ValueError('Unexpected archive member type')
            if hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()!=tree[path]:raise ValueError(f'Archive bytes differ from Git blob: {path}')
            if path in WINDOWS_INPUTS:
                item=WINDOWS_INPUTS[path]
                if sha(raw)!=item['gitBlobSha256'] or sha(raw.replace(b'\n',b'\r\n'))!=item['windowsSourceSha256']:raise ValueError(f'Pinned prompt input differs: {path}')
            header=notice(path) if path in changed else b''
            exported=header+raw
            merged={}
            if path=='.gitattributes':
                if attributes and not attributes.endswith(b'\n'):raise ValueError('Existing attributes need a final newline')
                exported=attributes+ATTR_BEGIN+header+raw+prompt_attributes()+ATTR_END
                merged={'attributesMerged':True,'repositoryPrefixBytes':len(attributes),'repositoryPrefixSha256':sha(attributes)}
            outputs[path]=exported
            inventory.append({'path':path,'gitMode':'120000' if member.issym() else ('100755' if member.mode&0o111 else '100644'),'gitBlob':tree[path],'sourceSha256':sha(raw),'publishedSha256':sha(exported),'bytes':len(exported),'modified':path in changed,'noticeOnly':bool(header) and not merged,**merged})
    for name in ['LICENSE','NOTICE']:outputs['upstream/'+name]=git(source,'show',PATCHED+':'+name)
    outputs[PATCH_PATH]=patch
    outputs['upstream/source-inventory.json']=encode({'schema':2,'sourceCommit':PATCHED,'files':inventory,'excluded':excluded,'symlinks':symlinks})
    outputs['SOURCE_BASELINE.json']=encode({'schema':2,'upstream':'https://github.com/openai/codex','officialVersion':'0.167.0','previousPublicCommit':PREVIOUS_PUBLIC,'officialCommit':OFFICIAL,'patchedCommit':PATCHED,'officialTree':git(source,'rev-parse',OFFICIAL+'^{tree}').decode().strip(),'patchedTree':git(source,'rev-parse',PATCHED+'^{tree}').decode().strip(),'selectedPrefixes':PREFIXES,'patchPath':PATCH_PATH,'patchSha256':sha(patch),'sourceInventorySha256':sha(outputs['upstream/source-inventory.json']),'windowsPromptInputsPath':WINDOWS_INPUTS_PATH,'windowsPromptInputsSha256':sha((root/WINDOWS_INPUTS_PATH).read_bytes()),'sourceFileCount':len(inventory),'publishedBytes':sum(p['bytes'] for p in inventory),'modifiedNoticeDate':DATE,'modifiedNoticeLabel':LABEL,'modifiedFiles':[p for p in inventory if p['modified']],'preservedRepositoryDocuments':existing,'limitations':['This public export has not been built or run as a fresh public checkout.','Operational checks, logs, credentials, binaries, local receipts and local Git history are excluded.','Tracked compressed schemas and other upstream source resources remain build inputs, not installable binaries.','Cargo.lock and workspace path dependencies are preserved; no machine-local cache/config overrides are exported.']})
    publish(root,outputs,previous)
    if os.name=='nt':
        for path,item in WINDOWS_INPUTS.items():
            target=checked_target(root,path);data=outputs[path].replace(b'\n',b'\r\n')
            if sha(data)!=item['windowsSourceSha256']:raise ValueError(f'Windows prompt transform differs: {path}')
            if target.read_bytes()!=data:target.write_bytes(data)
    assert existing=={p:sha((root/p).read_bytes()) for p in PROTECTED}
    print(json.dumps({'status':'PASS','sourceFiles':len(inventory),'modifiedFiles':sum(p['modified'] for p in inventory),'excludedAutomationFiles':len(excluded),'symlinks':symlinks,'patchSha256':sha(patch)}))
if __name__=='__main__':main()
