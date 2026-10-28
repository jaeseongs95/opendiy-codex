# Background collector component check

This check reads the candidate's exact collector/pause helpers, output definitions, bounded buffer and committed collector tests. It extracts production bodies unchanged; the red copy removes only early-yield observation. It does not compile core/TUI/RPC or spawn the tested shell process.

Run only in an externally authorized resource window:

```powershell
python checks/run-background-collector-proof.py --thin-lto --prepare-only --source-root ../cli-integration-20261028
python checks/run-background-collector-proof.py --thin-lto --source-root ../cli-integration-20261028
```

One script execution compiles the red and green small harnesses sequentially, once each; there is no retry or Cargo invocation. The compiler timeout is 120 seconds and each test executable has a 15 second limit. `--prepare-only` runs neither compiler nor tests.

The exact `--extern` filenames come from `../official-cli-patch-20261028/compiled-graph-v1.json`, rather than glob ordering. Selected Tokio/tracing hashes are checked before linking; every linked library's observed hash and the graph hash are recorded. Process-local build environment values are reused with Cargo paths redirected to unused paths under this worktree.

The frozen dependencies contain LLVM bitcode. Required compiler flags are `--test --edition=2024 --target x86_64-pc-windows-msvc -C opt-level=0 -C lto=thin -C codegen-units=2 -C debuginfo=0 -C target-feature=+crt-static`, the existing MSVC linker, exact graph dependency directories and Windows import-library paths. Raw argv, input hashes, compile/run output and receipts stay under `checks/collector-proof-thinlto-{red,green}`. The previous failed non-LTO attempt remains under `checks/collector-proof-green`.

Green must preserve `before\nduring\n` in one response, retain live/open handles, and allow later `after\n` collection with the full transcript intact. Red must fail the same one-second wakeup assertion. This component result cannot establish PID/PTY/environment continuity, cancellation/timeout manager races, terminal keys, backend refusal, integrated compilation or overall feature completion; those remain separate required tests.
