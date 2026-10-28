# Windows CLI packaging tool

Creates a **new local package** with the built CLI and matching official helper resources. It preserves the official installation and uses `--no-daemon` to select the patched embedded backend. It does not change PATH, authentication, daemon state, or user configuration.

```text
python package_cli.py --source CHECKOUT --freeze source-freeze.json --binary BUILT_CODEX_EXE --vendor OFFICIAL_VENDOR_DIRECTORY --output NEW_DIRECTORY --version PATCHED_VERSION
```

The source-freeze manifest contains `candidateCommit` and `files` entries with relative `path` and `sha256`. The tool checks HEAD, tracked changes, manifest hashes, helper base version, executable version, and copied file hashes. A complete-copy receipt is written last. A receipt does not prove a successful build, independent audit, or correct backend operation; those checks remain required before installation.

Test the package and launcher with a disposable argv fixture, not a user profile:

```text
rustc --edition=2024 argv_fixture.rs -o argv-fixture.exe
```

Set `PACKAGE_FIXTURE_EXE` to that executable's absolute path, then run `python -m unittest -v test_package_cli`. Tests use temporary Git repositories and new output directories; the fixture handles no real Codex requests. Source files and build tools may be published; generated packages, executables, and local receipts are not source deliverables.
