<!-- Modified for OpenDIY Codex R28 (2026-10-29); see MODIFICATIONS.md. -->
# Windows CLI packaging tool

Creates a **new local package** with the built CLI and matching official helper resources. Packaging does not change PATH, authentication, daemon state, or user configuration.

`codex-patched.cmd` forwards arguments unchanged to the adjacent executable, including `remote-control` and its global options. Its interactive mode uses the normal managed-backend selection. The executable version alone does not prove that the managed server is patched: a previously installed server may still be selected. Installing and checking the matching backend is a separate explicit step (`bin\codex.exe app-server daemon update --from-cli`) that can restart a shared server and interrupt other clients. Verify the running server path and binary hash before claiming patch activation or mobile interoperability.

`codex-local.cmd` explicitly adds `--no-daemon` for an isolated embedded backend. It cannot be used for managed `remote-control` commands or as evidence that a mobile client and local TUI share a live backend. Packaging does not perform enrollment, pairing, or installation. Agent Grid's panel-owned WebSocket servers also require a separate backend-identity and lifecycle check; sharing a profile directory is not sufficient.

```text
python package_cli.py --source CHECKOUT --freeze source-freeze.json --binary BUILT_CODEX_EXE --vendor OFFICIAL_VENDOR_DIRECTORY --output NEW_DIRECTORY --version PATCHED_VERSION
```

The source-freeze manifest contains `candidateCommit` and `files` entries with relative `path` and `sha256`. The tool checks HEAD, tracked changes, manifest hashes, helper base version, executable version, and copied file hashes. A complete-copy receipt is written last. A receipt does not prove a successful build, independent audit, or correct backend operation; those checks remain required before installation.

Test the package and launcher with a disposable argv fixture, not a user profile:

```text
rustc --edition=2024 argv_fixture.rs -o argv-fixture.exe
```

Set `PACKAGE_FIXTURE_EXE` to that executable's absolute path, then run `python -m unittest -v test_package_cli`. Tests use temporary Git repositories and new output directories; the fixture handles no real Codex requests. Source files and build tools may be published; generated packages, executables, and local receipts are not source deliverables.
