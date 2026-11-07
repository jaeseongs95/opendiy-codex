"""Small synthetic checks for verifier launch and cross-platform symlink handling."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT=Path(__file__).with_name('verify.py')
SPEC=importlib.util.spec_from_file_location('source_verifier',SCRIPT)
VERIFY=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)
EXPORT_SPEC=importlib.util.spec_from_file_location('source_exporter',SCRIPT.with_name('export.py'))
EXPORT=importlib.util.module_from_spec(EXPORT_SPEC)
EXPORT_SPEC.loader.exec_module(EXPORT)

class VerifierChecks(unittest.TestCase):
    def fixture(self):
        temporary=tempfile.TemporaryDirectory(prefix='source-import-verifier-')
        self.addCleanup(temporary.cleanup)
        root=Path(temporary.name);path=root/VERIFY.LICENSE_LINK
        path.parent.mkdir(parents=True)
        (path.parent/'COPYING').write_bytes(b'synthetic license fixture')
        return root,path
    def pinned_repository(self):
        root,_=self.fixture()
        old=root/'codex-rs/old.rs';old.write_bytes(b'published follow-up')
        (root/'codex-rs/new.rs').write_bytes(b'published new test')
        def git(*args):
            return subprocess.check_output(['git','-c','commit.gpgsign=false','-c','user.name=Synthetic','-c','user.email=test@example.invalid','-C',str(root),*args],stderr=subprocess.DEVNULL)
        git('init','-q');git('add','--','codex-rs');git('commit','-qm','synthetic')
        return root,old,git('rev-parse','HEAD').decode().strip()
    def test_update_accepts_only_git_anchored_public_followups(self):
        root,old,commit=self.pinned_repository()
        with patch.object(EXPORT,'PREVIOUS_PUBLIC',commit):
            previous=EXPORT.pinned_previous_source(root,{'codex-rs/old.rs':EXPORT.sha(b'original export')})
        self.assertEqual(previous['codex-rs/old.rs'],EXPORT.sha(b'published follow-up'))
        self.assertEqual(previous['codex-rs/new.rs'],EXPORT.sha(b'published new test'))
    def test_update_preserves_unimported_supplementary_patch(self):
        root,old,_=self.pinned_repository()
        extra=root/'patches/next.patch';extra.parent.mkdir();extra.write_bytes(b'separate future candidate')
        subprocess.run(['git','-C',str(root),'add','--','patches/next.patch'],check=True,stdout=subprocess.DEVNULL)
        subprocess.run(['git','-c','commit.gpgsign=false','-c','user.name=Synthetic','-c','user.email=test@example.invalid','-C',str(root),'commit','-qm','supplementary patch'],check=True)
        commit=subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip()
        with patch.object(EXPORT,'PREVIOUS_PUBLIC',commit):
            previous=EXPORT.pinned_previous_source(root,{'codex-rs/old.rs':EXPORT.sha(old.read_bytes())})
        self.assertNotIn('patches/next.patch',previous)
        self.assertIn('codex-rs/new.rs',previous)
        EXPORT.publish(root,{'codex-rs/old.rs':b'updated','codex-rs/new.rs':b'published new test'},previous)
        self.assertEqual(extra.read_bytes(),b'separate future candidate')
    def test_update_rejects_working_edits_to_pinned_public_followups(self):
        root,old,commit=self.pinned_repository();old.write_bytes(b'unreviewed edit')
        with patch.object(EXPORT,'PREVIOUS_PUBLIC',commit),self.assertRaises(subprocess.CalledProcessError):
            EXPORT.pinned_previous_source(root,{'codex-rs/old.rs':EXPORT.sha(b'original export')})
        self.assertEqual(old.read_bytes(),b'unreviewed edit')
    def test_windows_placeholder(self):
        root,path=self.fixture();path.write_bytes(b'COPYING')
        self.assertEqual(VERIFY.read_source(root,{'path':VERIFY.LICENSE_LINK,'gitMode':'120000'}),b'COPYING')
    def test_actual_symlink(self):
        root,path=self.fixture()
        try:os.symlink('COPYING',path)
        except OSError:self.skipTest('Creating actual symlinks is unavailable on this host')
        self.assertEqual(VERIFY.read_source(root,{'path':VERIFY.LICENSE_LINK,'gitMode':'120000'}),b'COPYING')
        with self.assertRaises(ValueError):VERIFY.read_source(root,{'path':VERIFY.LICENSE_LINK,'gitMode':'100644'})
    def test_readlink_branch_with_synthetic_link(self):
        root,path=self.fixture();path.write_bytes(b'COPYING')
        with patch.object(Path,'is_symlink',lambda candidate:candidate==path),patch.object(VERIFY.os,'readlink',return_value='COPYING'):
            self.assertEqual(VERIFY.read_source(root,{'path':VERIFY.LICENSE_LINK,'gitMode':'120000'}),b'COPYING')
            with self.assertRaises(ValueError):VERIFY.read_source(root,{'path':VERIFY.LICENSE_LINK,'gitMode':'100644'})
    def test_wrong_placeholder_target(self):
        root,path=self.fixture();path.write_bytes(b'../outside')
        with self.assertRaises(ValueError):VERIFY.read_source(root,{'path':VERIFY.LICENSE_LINK,'gitMode':'120000'})
    def test_parent_escape_and_unlisted_link(self):
        root,path=self.fixture();path.write_bytes(b'COPYING')
        for relative in ['../outside','unlisted/LICENSE']:
            with self.assertRaises(ValueError):VERIFY.read_source(root,{'path':relative,'gitMode':'120000'})
    def test_update_refuses_changed_import_before_any_write(self):
        root,_=self.fixture();old=root/'codex-rs/old.rs';old.write_bytes(b'user edit')
        new=root/'codex-rs/new.rs'
        with self.assertRaisesRegex(ValueError,'Previous import changed'):
            EXPORT.publish(root,{'codex-rs/new.rs':b'new'},{'codex-rs/old.rs':EXPORT.sha(b'original')})
        self.assertFalse(new.exists());self.assertEqual(old.read_bytes(),b'user edit')
    def test_update_refuses_uninventoried_collision_before_any_write(self):
        root,_=self.fixture();old=root/'codex-rs/old.rs';old.write_bytes(b'original')
        collision=root/'codex-rs/new.rs';collision.write_bytes(b'unrelated')
        with self.assertRaisesRegex(ValueError,'refusing overwrite'):
            EXPORT.publish(root,{'codex-rs/old.rs':b'updated','codex-rs/new.rs':b'new'},{'codex-rs/old.rs':EXPORT.sha(b'original')})
        self.assertEqual(old.read_bytes(),b'original');self.assertEqual(collision.read_bytes(),b'unrelated')
    def test_update_replaces_and_retires_only_verified_source(self):
        root,_=self.fixture();old=root/'codex-rs/old.rs';old.write_bytes(b'old')
        retired=root/'codex-rs/retired.rs';retired.write_bytes(b'retired')
        readme=root/'README.md';readme.write_bytes(b'user readme')
        EXPORT.publish(root,{'codex-rs/old.rs':b'updated','codex-rs/new.rs':b'new'},
            {'codex-rs/old.rs':EXPORT.sha(b'old'),'codex-rs/retired.rs':EXPORT.sha(b'retired')})
        self.assertEqual(old.read_bytes(),b'updated');self.assertFalse(retired.exists())
        self.assertEqual((root/'codex-rs/new.rs').read_bytes(),b'new');self.assertEqual(readme.read_bytes(),b'user readme')
    def test_only_pinned_prompt_inputs_normalize_checkout_eol(self):
        path=next(iter(EXPORT.WINDOWS_INPUTS))
        self.assertEqual(EXPORT.canonical_bytes(path,b'a\r\nb\r\n'),b'a\nb\n')
        self.assertEqual(EXPORT.canonical_bytes('codex-rs/schema.json.zst',b'\x00\r\n'),b'\x00\r\n')
        self.assertNotIn(b'eol=crlf',EXPORT.prompt_attributes())
        self.assertEqual(EXPORT.prompt_attributes().count(b' text\n'),40)
    def test_generated_typescript_preserves_actual_rust_prefix_contract(self):
        root=SCRIPT.resolve().parents[2]
        inventory=__import__('json').loads((root/'upstream/source-inventory.json').read_bytes())
        fixtures=[item for item in inventory['files'] if item['modified'] and item['path'].startswith('codex-rs/app-server-protocol/schema/typescript/') and item['path'].endswith('.ts')]
        self.assertEqual({item["path"] for item in fixtures}, {
            'codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts',
            'codex-rs/app-server-protocol/schema/typescript/OpenDiyCapabilities.ts',
            'codex-rs/app-server-protocol/schema/typescript/index.ts',
            'codex-rs/app-server-protocol/schema/typescript/InitializeResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/CompletedChildCleanupResult.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/CompletedChildSnapshot.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/MemoryConsolidationState.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/MemoryNoteScope.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/SubagentControlStatus.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/SubagentManagementAction.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/SubagentManagementAgent.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/SubagentManagementCapabilities.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/SubagentManagementStatus.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/SubagentMutationState.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadCleanupCompletedParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadCleanupCompletedResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadCompactStartIfIdleParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadPluginReloadParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadPluginReloadResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadResumeParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadResumeResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadSettings.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadSubagentsControlParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadSubagentsControlResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadSubagentsListParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadSubagentsListResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/ThreadSubscribePassiveParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/TurnStartIfIdleParams.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/TurnStartIfIdleResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/index.ts',
        })
        header=VERIFY.generated_ts_header(root)
        for item in fixtures:
            with self.subTest(path=item['path']):
                data=VERIFY.read_source(root,item)
                VERIFY.check_generated_ts_prefix(item['path'],data,header)
                self.assertEqual(EXPORT.notice(item['path']),b'')
    def test_generated_typescript_rejects_a_notice_before_the_real_header(self):
        header=VERIFY.generated_ts_header(SCRIPT.resolve().parents[2])
        with self.assertRaisesRegex(AssertionError,'Generated TypeScript prefix differs'):
            VERIFY.check_generated_ts_prefix('codex-rs/app-server-protocol/schema/typescript/Fixture.ts',b'// local modification notice\n'+header+b'export type Fixture = string;\n',header)
    def test_source_and_data_notice_formats(self):
        self.assertTrue(EXPORT.notice('codex-rs/core/src/lib.rs').startswith(b'//'))
        self.assertTrue(EXPORT.notice('tools/windows-cli/package_cli.py').startswith(b'#'))
        self.assertEqual(EXPORT.notice('codex-rs/schema.json'),b'')
        self.assertEqual(EXPORT.notice('codex-rs/schema.json.zst'),b'')
        self.assertEqual(EXPORT.notice('codex-rs/expected.snap'),b'')
    def test_r33_notice_is_limited_to_reviewed_followups(self):
        for path in ['codex-rs/tui/src/app/replay_filter.rs',
                     'codex-rs/tui/src/app/thread_routing.rs',
                     'codex-rs/tui/src/app/tests/buffered_replay.rs',
                     'codex-rs/app-server-daemon/src/backend/mod.rs',
                     'codex-rs/app-server-daemon/src/backend/pid.rs',
                     'codex-rs/app-server-daemon/src/backend/pid_tests.rs',
                     'codex-rs/app-server-daemon/src/backend/pid_start.rs',
                     'codex-rs/app-server-daemon/src/backend/windows.rs',
                     'codex-rs/app-server-daemon/src/backend/windows_spawn.rs',
                     'codex-rs/app-server-daemon/src/backend/windows_tests.rs',
                     'codex-rs/app-server-daemon/src/lib.rs',
                     'codex-rs/app-server-daemon/src/prepare_install.rs',
                     'codex-rs/app-server-daemon/src/prepare_install_tests.rs',
                     'codex-rs/app-server-daemon/src/status.rs',
                     'codex-rs/cli/src/daemon_telemetry.rs',
                     'codex-rs/codex-api/tests/cache_batch_boundary.rs',
                     'codex-rs/diagnostics/src/cache_diagnostics.rs']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path), b'// Modified for OpenDIY Codex R38 (2026-11-06); see MODIFICATIONS.md.\n' if path == 'codex-rs/tui/src/app/thread_routing.rs' else b'// Modified for OpenDIY Codex R37 (2026-11-04); see MODIFICATIONS.md.\n' if path in {'codex-rs/tui/src/app/replay_filter.rs', 'codex-rs/tui/src/app/thread_routing.rs', 'codex-rs/tui/src/app/tests/buffered_replay.rs', 'codex-rs/app-server-daemon/src/lib.rs', 'codex-rs/app-server-daemon/src/prepare_install.rs'} else (b'// Modified for OpenDIY Codex R36 (2026-11-03); see MODIFICATIONS.md.\n' if path == 'codex-rs/tui/src/app/thread_routing.rs' else b'// Modified for OpenDIY Codex R35 (2026-11-03); see MODIFICATIONS.md.\n' if path in {'codex-rs/app-server-daemon/src/lib.rs', 'codex-rs/diagnostics/src/cache_diagnostics.rs'} else b'// Modified for OpenDIY Codex R33 (2026-11-02); see MODIFICATIONS.md.\n'))
        for path in ['codex-rs/app-server-daemon/README.md',
                     'docs/native-cli-r33-cache-observation.ko.md',
                     'tools/windows-cli/README.md',
                     'tools/windows-cli/UPDATE.ko.md']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path),b'<!-- Modified for OpenDIY Codex R33 (2026-11-02); see MODIFICATIONS.md. -->\n')
        for path in ['codex-rs/app-server-daemon/Cargo.toml']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path),b'# Modified for OpenDIY Codex R33 (2026-11-02); see MODIFICATIONS.md.\n')
        for path in ['tools/windows-cli/update.ps1', 'tools/windows-cli/test_update.ps1',
                     'tools/windows-cli/test_update_output.py']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path),b'# Modified for OpenDIY Codex R34 (2026-11-03); see MODIFICATIONS.md.\n')
        self.assertEqual(EXPORT.notice('tools/windows-cli/update.vbs'),b"' Modified for OpenDIY Codex R33 (2026-11-02); see MODIFICATIONS.md.\n")
        self.assertEqual(EXPORT.notice('codex-rs/cli/src/bin/opendiy_codex_launcher.rs'),b'// Modified for OpenDIY Codex R33 (2026-11-02); see MODIFICATIONS.md.\n')
        self.assertEqual(EXPORT.notice('codex-rs/app-server-daemon/scripts/install/install.ps1'),b'')
        self.assertEqual(EXPORT.notice('codex-rs/core/src/lib.rs'),b'// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.\n')
    def test_runtime_markdown_prompts_do_not_receive_local_notices(self):
        for path in EXPORT.WINDOWS_INPUTS:
            with self.subTest(path=path):
                if path.endswith(".md"):
                    self.assertEqual(EXPORT.notice(path), b"")
        self.assertTrue(EXPORT.notice("docs/native-cli-r27.ko.md").startswith(b"<!--"))
    def test_python_optimized_mode_rejected(self):
        process=subprocess.run([sys.executable,'-B','-O',str(SCRIPT)],capture_output=True,text=True)
        self.assertNotEqual(process.returncode,0)
        self.assertIn('without Python optimization',process.stderr)

    def test_r34_notice_marks_only_reviewed_startup_and_upstream_files(self):
        for path in ['codex-rs/cli/src/main.rs', 'codex-rs/tui/src/main.rs',
                     'codex-rs/tui/src/lib.rs', 'codex-rs/tui/src/lib_tests.rs',
                     'codex-rs/tui/src/tui.rs', 'codex-rs/tui/src/tui_panic_tests.rs',
                     'codex-rs/cli/tests/daemon_startup.rs',
                     'codex-rs/tui/src/app_server_session/history.rs',
                     'codex-rs/tui/src/chatwidget.rs', 'codex-rs/tui/src/keymap.rs',
                     'codex-rs/tui/src/app/tests/startup_defaults_tests.rs']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path), b'// Modified for OpenDIY Codex R38 (2026-11-06); see MODIFICATIONS.md.\n' if path in {'codex-rs/tui/src/lib.rs', 'codex-rs/cli/src/main.rs', 'codex-rs/cli/tests/daemon_startup.rs'} else b'// Modified for OpenDIY Codex R37 (2026-11-04); see MODIFICATIONS.md.\n' if path in {'codex-rs/tui/src/tui.rs'} else (b'// Modified for OpenDIY Codex R35 (2026-11-03); see MODIFICATIONS.md.\n' if path == 'codex-rs/tui/src/keymap.rs' else b'// Modified for OpenDIY Codex R34 (2026-11-03); see MODIFICATIONS.md.\n'))
        for path in ['codex-rs/Cargo.toml', 'codex-rs/Cargo.lock', 'codex-rs/cli/Cargo.toml']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path), b'# Modified for OpenDIY Codex R39 (2026-11-07); see MODIFICATIONS.md.\n' if path in {'codex-rs/Cargo.toml', 'codex-rs/Cargo.lock'} else b'# Modified for OpenDIY Codex R35 (2026-11-03); see MODIFICATIONS.md.\n' if path == 'codex-rs/cli/Cargo.toml' else b'# Modified for OpenDIY Codex R36 (2026-11-03); see MODIFICATIONS.md.\n')

    def test_r37_export_scope_and_notices(self):
        expected_docs={
            'docs/r37-agent-selection-cases.ko.md',
            'docs/r37-cold-cleanup-cases.ko.md',
            'docs/r37-config-cases.ko.md',
            'docs/r37-daemon-cases.ko.md',
            'docs/r37-memory-cases.ko.md',
            'docs/r37-memory-transport-cases.ko.md',
            'docs/r37-plugin-cache-cases.ko.md',
            'docs/r37-replay-compact-cases.ko.md',
        }
        self.assertTrue(expected_docs.issubset(EXPORT.PREFIXES))
        self.assertNotIn('docs/r37-cache-observation-status.ko.md',EXPORT.PREFIXES)
        self.assertIn('sdk/python/src/openai_codex/generated/v2_all.py',EXPORT.PREFIXES)
        self.assertEqual(EXPORT.notice('codex-rs/core-plugins/src/store.rs'),b'// Modified for OpenDIY Codex R38 (2026-11-06); see MODIFICATIONS.md.\n')
        self.assertEqual(EXPORT.notice('sdk/python/src/openai_codex/generated/v2_all.py'),b'# Modified for OpenDIY Codex R37 (2026-11-04); see MODIFICATIONS.md.\n')

    def test_r38_scope_does_not_expand_into_operational_artifacts(self):
        paths={
            'docs/native-cli-r38.ko.md',
            'docs/r38-daemon-test-cases.ko.md',
            'docs/r38-plugin-cache-test-cases.ko.md',
            'docs/r38-stop-hook-test-cases.ko.md',
            'docs/r38-agent-main-return-test-cases.ko.md',
            'scripts/install/normalize_stop_hook_config.py',
            'scripts/install/test_normalize_stop_hook_config.py',
        }
        self.assertTrue(paths.issubset(EXPORT.PREFIXES))
        self.assertNotIn('docs/r37-cache-observation-status.ko.md',EXPORT.PREFIXES)
        self.assertNotIn('checks',EXPORT.PREFIXES)
        for path in ['codex-rs/tui/src/app_server_connection.rs',
                     'codex-rs/tui/src/startup_orchestration.rs']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path),b'// Modified for OpenDIY Codex R38 (2026-11-06); see MODIFICATIONS.md.\n')
        self.assertEqual(EXPORT.notice('scripts/install/normalize_stop_hook_config.py'),b'# Modified for OpenDIY Codex R38 (2026-11-06); see MODIFICATIONS.md.\n')
        self.assertEqual(EXPORT.notice('codex-rs/app-server-daemon/src/prepare_install.rs'),b'// Modified for OpenDIY Codex R37 (2026-11-04); see MODIFICATIONS.md.\n')

    def test_r39_0167_pins_and_runtime_prompt_catalog(self):
        self.assertEqual(EXPORT.OFFICIAL,'086ccc833bc9cf01024ad2dd6af7dbd421b6bf9a')
        self.assertEqual(EXPORT.PATCHED,'c4399dce957bb63fe1fdc276aa4306c06f6cf47f')
        self.assertEqual(EXPORT.PREVIOUS_PUBLIC,'e54da9ee059278403684061bb86e1e7072c16165')
        self.assertEqual(EXPORT.PATCH_PATH,'upstream/official-0.167.0.patch')
        catalog=__import__('json').loads((SCRIPT.resolve().parents[2]/EXPORT.WINDOWS_INPUTS_PATH).read_bytes())
        self.assertEqual(catalog['sourceCommit'],'c4399dce957bb63fe1fdc276aa4306c06f6cf47f')
        paths=[item['path'] for item in catalog['files']]
        self.assertEqual(len(paths),40)
        self.assertEqual(len(set(paths)),40)
        self.assertEqual(EXPORT.sha(('\n'.join(sorted(paths))+'\n').encode()),'3f0f64f1fcf91b22c111f8d97da372e038eff083d3172e61560f0c0500b79e24')
        self.assertIn('codex-rs/prompts/templates/review/exit_interrupted.xml',paths)
        self.assertIn('codex-rs/prompts/templates/review/exit_success.xml',paths)


    def test_r39_compact_notices_are_limited_to_reviewed_source(self):
        expected={
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
        }
        self.assertEqual({path for path, label_date in EXPORT.NOTICE_OVERRIDES.items()
                          if label_date == ('R39', '2026-11-07')}, expected)
        for path in expected:
            with self.subTest(path=path):
                prefix = b'//' if path.endswith('.rs') else b'#'
                self.assertEqual(EXPORT.notice(path), prefix + b' Modified for OpenDIY Codex R39 (2026-11-07); see MODIFICATIONS.md.\n')

    def test_r39_compact_prompt_has_exact_runtime_bytes(self):
        relative='codex-rs/prompts/templates/compact/prompt.md'
        data=(SCRIPT.resolve().parents[2]/relative).read_bytes()
        canonical=EXPORT.canonical_bytes(relative,data)
        self.assertEqual(EXPORT.notice(relative),b'')
        self.assertNotIn(b'<!-- Modified',canonical)
        self.assertEqual(EXPORT.sha(canonical),EXPORT.WINDOWS_INPUTS[relative]['gitBlobSha256'])
        if os.name=='nt':
            self.assertEqual(EXPORT.sha(data),EXPORT.WINDOWS_INPUTS[relative]['windowsSourceSha256'])

if __name__=='__main__':unittest.main()
