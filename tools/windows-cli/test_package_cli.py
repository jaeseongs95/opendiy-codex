"""Local package tests. These are not tests of the patched CLI's runtime."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from package_cli import digest, package


class PackageTests(unittest.TestCase):
    def setUp(self):
        fixture = os.environ.get("PACKAGE_FIXTURE_EXE")
        if not fixture:
            self.skipTest("Set PACKAGE_FIXTURE_EXE to the local argv fixture executable")
        self.temporary = tempfile.TemporaryDirectory(prefix="codex-package-한글 ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.asset = self.source / "asset.md"
        self.asset.write_bytes(b"frozen prompt\r\n")
        (self.source / "other.txt").write_bytes(b"tracked but not in changed-file freeze")
        for args in (
            ["init", "-q"], ["add", "asset.md", "other.txt"],
            ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
             "commit", "-qm", "fixture"],
        ):
            subprocess.run(["git", "-C", str(self.source), *args], check=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        commit = subprocess.check_output(
            ["git", "-C", str(self.source), "rev-parse", "HEAD"], text=True
        ).strip()
        self.freeze = self.root / "freeze.json"
        self.freeze.write_text(json.dumps({
            "candidateCommit": commit,
            "files": [{"path": "asset.md", "sha256": digest(self.asset)}],
        }), encoding="utf-8")
        self.binary = Path(fixture)
        self.vendor = self.root / "vendor"
        (self.vendor / "bin").mkdir(parents=True)
        (self.vendor / "bin/codex.exe").write_bytes(b"official binary must be replaced")
        (self.vendor / "helper.dat").write_bytes(b"preserved helper")
        self.metadata = self.vendor / "codex-package.json"
        self.metadata.write_text(json.dumps({
            "layoutVersion": 1, "version": "0.165.0", "entrypoint": "bin/codex.exe",
        }), encoding="utf-8")
        self.output = self.root / "패치 package"

    def build(self, version="0.165.0+test"):
        return package(self.source, self.freeze, self.binary, self.vendor, self.output, version)

    def test_copies_helpers_replaces_binary_and_records_relative_paths(self):
        receipt = self.build()
        self.assertEqual(digest(self.output / "bin/codex.exe"), digest(self.binary))
        self.assertEqual((self.output / "helper.dat").read_bytes(), b"preserved helper")
        self.assertEqual(self.asset.read_bytes(), b"frozen prompt\r\n")
        self.assertEqual(receipt, json.loads((self.output / "package-receipt.json").read_text()))
        for item in receipt["files"]:
            self.assertFalse(Path(item["path"]).is_absolute())
            self.assertEqual(item["sha256"], digest(self.output / item["path"]))

    def test_launcher_uses_adjacent_binary_and_preserves_arguments_exit(self):
        self.build()
        captured = self.root / "argv.json"
        env = dict(os.environ, PACKAGE_FIXTURE_ARGV=str(captured))
        result = subprocess.run(
            'cmd.exe /d /c codex-patched.cmd --literal "two words" 한글',
            cwd=self.output, env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertEqual(captured.read_text(encoding="utf-8").splitlines(),
                         ["--literal", "two words", "한글"])

    def test_remote_command_and_global_options_are_not_rewritten(self):
        self.build()
        captured = self.root / "remote-argv.txt"
        env = dict(os.environ, PACKAGE_FIXTURE_ARGV=str(captured))
        result = subprocess.run(
            'cmd.exe /d /c codex-patched.cmd -c "key=value" remote-control status --json',
            cwd=self.output, env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertEqual(captured.read_text(encoding="utf-8").splitlines(),
                         ["-c", "key=value", "remote-control", "status", "--json"])

    def test_local_launcher_explicitly_selects_embedded_mode(self):
        self.build()
        captured = self.root / "local-argv.txt"
        env = dict(os.environ, PACKAGE_FIXTURE_ARGV=str(captured))
        result = subprocess.run(
            'cmd.exe /d /c codex-local.cmd --literal "two words" 한글',
            cwd=self.output, env=env,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(result.returncode, 17, result.stderr)
        self.assertEqual(captured.read_text(encoding="utf-8").splitlines(),
                         ["--no-daemon", "--literal", "two words", "한글"])

    def test_existing_output_is_not_overwritten(self):
        self.output.mkdir()
        sentinel = self.output / "keep.txt"
        sentinel.write_bytes(b"do not replace")
        with self.assertRaises(ValueError):
            self.build()
        self.assertEqual(sentinel.read_bytes(), b"do not replace")

    def test_changed_frozen_source_is_rejected_before_output(self):
        self.asset.write_bytes(b"different source")
        with self.assertRaisesRegex(ValueError, "Source changed"):
            self.build()
        self.assertFalse(self.output.exists())

    def test_source_head_mismatch_is_rejected(self):
        data = json.loads(self.freeze.read_text())
        data["candidateCommit"] = "0" * 40
        self.freeze.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Source HEAD"):
            self.build()
        self.assertFalse(self.output.exists())

    def test_changes_outside_changed_file_freeze_are_rejected(self):
        (self.source / "other.txt").write_bytes(b"changed build input")
        with self.assertRaisesRegex(ValueError, "Source differs"):
            self.build()
        self.assertFalse(self.output.exists())

    def test_helper_base_version_mismatch_is_rejected(self):
        data = json.loads(self.metadata.read_text())
        data["version"] = "0.164.0"
        self.metadata.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "helper package"):
            self.build()
        self.assertFalse(self.output.exists())

    def test_binary_version_mismatch_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Unexpected CLI version"):
            self.build("0.165.0+different")
        self.assertFalse(self.output.exists())

    def test_vendor_output_recursion_is_rejected(self):
        self.output = self.vendor / "nested-output"
        with self.assertRaises(ValueError):
            self.build()
        self.assertFalse(self.output.exists())

    def test_frozen_path_cannot_escape_source(self):
        outside = self.root / "outside.txt"
        outside.write_bytes(b"outside")
        data = json.loads(self.freeze.read_text())
        data["files"] = [{"path": "../outside.txt", "sha256": digest(outside)}]
        self.freeze.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "Not a package file"):
            self.build()
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
