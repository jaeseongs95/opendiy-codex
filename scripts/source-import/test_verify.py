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

class VerifierChecks(unittest.TestCase):
    def fixture(self):
        temporary=tempfile.TemporaryDirectory(prefix='source-import-verifier-')
        self.addCleanup(temporary.cleanup)
        root=Path(temporary.name);path=root/VERIFY.LICENSE_LINK
        path.parent.mkdir(parents=True)
        (path.parent/'COPYING').write_bytes(b'synthetic license fixture')
        return root,path
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
    def test_python_optimized_mode_rejected(self):
        process=subprocess.run([sys.executable,'-B','-O',str(SCRIPT)],capture_output=True,text=True)
        self.assertNotEqual(process.returncode,0)
        self.assertIn('without Python optimization',process.stderr)

if __name__=='__main__':unittest.main()
