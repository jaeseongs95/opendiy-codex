# Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
"""Prepare a separate Windows CLI package; never replace an existing installation."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def checked_child(root, relative):
    relative = Path(relative)
    child = (root / relative).resolve(strict=True)
    if relative.is_absolute() or not child.is_relative_to(root) or not child.is_file():
        raise ValueError(f"Not a package file: {relative}")
    return child


def package(source, freeze_file, binary, vendor, output, expected_version):
    source, vendor = source.resolve(strict=True), vendor.resolve(strict=True)
    binary, freeze_file = binary.resolve(strict=True), freeze_file.resolve(strict=True)
    output = output.resolve()
    if output.exists() or output.is_relative_to(vendor):
        raise ValueError("Output must be a new directory outside the vendor package")
    freeze = json.loads(freeze_file.read_text(encoding="utf-8"))
    commit = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    if commit != freeze["candidateCommit"]:
        raise ValueError("Source HEAD differs from the frozen candidate")
    for entry in freeze["files"]:
        if digest(checked_child(source, entry["path"])) != entry["sha256"]:
            raise ValueError(f"Source changed after freeze: {entry['path']}")
    dirty = subprocess.run(["git", "-C", str(source), "diff", "--quiet", "HEAD", "--"])
    if dirty.returncode:
        raise ValueError(f"Source differs from its commit (git diff exit {dirty.returncode})")
    metadata = json.loads((vendor / "codex-package.json").read_text(encoding="utf-8"))
    if metadata.get("entrypoint") != "bin/codex.exe":
        raise ValueError("Expected the official Windows CLI package layout")
    if metadata.get("version") != expected_version.split("+", 1)[0]:
        raise ValueError("Official helper package differs from the CLI base version")
    binary_hash = digest(binary)
    # Check before creating output: a failed version check cannot look installed.
    version = subprocess.check_output([str(binary), "--version"], text=True).strip()
    if version != f"codex-cli {expected_version}":
        raise ValueError(f"Unexpected CLI version: {version}")
    helpers = []
    for item in sorted(vendor.rglob("*")):
        if not item.is_file():
            continue
        relative = item.relative_to(vendor)
        if relative.as_posix() in {"bin/codex.exe", "codex-package.json"}:
            continue
        helpers.append((relative, checked_child(vendor, relative), digest(item)))
    output.mkdir(parents=True)
    for relative, item, expected_hash in helpers:
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(item, target)
        if digest(target) != expected_hash:
            raise ValueError(f"Helper changed during copy: {relative}")
    target = output / "bin/codex.exe"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, target)
    if digest(target) != binary_hash:
        raise ValueError("CLI binary changed during copy")
    metadata["version"] = expected_version
    (output / "codex-package.json").write_text(
        json.dumps(metadata, indent=2) + "\n", encoding="utf-8", newline="\n"
    )
    (output / "codex-patched.cmd").write_text(
        '@echo off\n"%~dp0bin\\codex.exe" %*\nexit /b %errorlevel%\n',
        encoding="utf-8", newline="\r\n",
    )
    (output / "codex-local.cmd").write_text(
        '@echo off\n"%~dp0bin\\codex.exe" --no-daemon %*\nexit /b %errorlevel%\n',
        encoding="utf-8", newline="\r\n",
    )
    # Written last. Presence records a complete copy, not audit or runtime success.
    receipt = {
        "sourceCommit": commit,
        "sourceFreezeSha256": digest(freeze_file),
        "binarySha256": binary_hash,
        "version": version,
        "files": [
            {"path": item.relative_to(output).as_posix(), "sha256": digest(item)}
            for item in sorted(output.rglob("*")) if item.is_file()
        ],
    }
    (output / "package-receipt.json").write_text(
        json.dumps(receipt, indent=2) + "\n", encoding="utf-8", newline="\n"
    )
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "freeze", "binary", "vendor", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    result = package(args.source, args.freeze, args.binary, args.vendor, args.output, args.version)
    print(json.dumps({"sourceCommit": result["sourceCommit"], "version": result["version"]}))
