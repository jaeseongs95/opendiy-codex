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
        self.assertEqual(EXPORT.prompt_attributes().count(b' text\n'),38)
    def test_generated_typescript_preserves_actual_rust_prefix_contract(self):
        root=SCRIPT.resolve().parents[2]
        inventory=__import__('json').loads((root/'upstream/source-inventory.json').read_bytes())
        fixtures=[item for item in inventory['files'] if item['modified'] and item['path'].startswith('codex-rs/app-server-protocol/schema/typescript/') and item['path'].endswith('.ts')]
        self.assertEqual({item["path"] for item in fixtures}, {
            'codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts',
            'codex-rs/app-server-protocol/schema/typescript/InitializeResponse.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/CompletedChildCleanupResult.ts',
            'codex-rs/app-server-protocol/schema/typescript/v2/CompletedChildSnapshot.ts',
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
    def test_r33_notice_is_limited_to_replay_followup(self):
        for path in ['codex-rs/tui/src/app/replay_filter.rs',
                     'codex-rs/tui/src/app/thread_routing.rs',
                     'codex-rs/tui/src/app/tests/buffered_replay.rs']:
            with self.subTest(path=path):
                self.assertEqual(EXPORT.notice(path),b'// Modified for OpenDIY Codex R33 (2026-11-02); see MODIFICATIONS.md.\n')
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

if __name__=='__main__':unittest.main()
