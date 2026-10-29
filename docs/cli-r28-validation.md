# R28 validation — 2026-10-29

This page summarizes actual local checks of source `3a03cbdadf186d74af73fcc5825afc25d3d85f39`, based on official Codex `0.165.0` (`360319d8dee62d87e2d59fc0c21805d79519cfed`). It is not a release approval or a fresh public-checkout build claim. Raw logs, profiles, receipts and binaries remain private.

## Build and package

The Windows MSVC release build exited 0 in 72 minutes 37 seconds. It reused the existing build cache, with two Cargo jobs, optimization level 3, ThinLTO and four codegen units. The source freeze contains 384 changed files with separate working-byte and Git-blob hashes.

```sh
cargo build --profile release --offline --locked --target x86_64-pc-windows-msvc -j2 -p codex-cli --bin codex --message-format=json-render-diagnostics --timings
```

The executable reports `codex-cli 0.165.0+native-r28.20261029`; SHA-256 is `b39add11b36d6e494db5406ebfa741935ceb319410af2d9f7400ec574639a2b7`. A separate portable package contains that executable and matching official helper resources; all 47 file hashes were checked. Version/help, remote-control help, app-server help and exec help each exited 0. Packaging does not install the shared daemon or prove backend identity.

## Actual final-executable checks

Tests created disposable profiles, disabled plugins only in those profiles, removed inherited account/provider overrides, and used synthetic responses from loopback servers. Original authentication, configuration, shared daemon and running user tasks were preserved.

| Boundary | Observed result |
| --- | --- |
| Capacity failure, then success | Two requests, expected acknowledgement, exit 0 |
| Capacity failure after partial output | One request, no replay, exit 1 |
| Retry-After 300 seconds exceeds retry window | One request, exit 1 |
| Ordinary error-exit diagnostics | Complete local log batches in both failure cases |
| Windows CTRL_C_EVENT to an owned test console | One request, turn/interrupt span completed, exit 1, complete local log batch |
| Export destination blocked by a regular file | Export failure recorded; successful command still exited 0; complete local batches retained |
| Next execution after destination recovery | Two previous batches published with identical filenames and hashes; their local originals removed |
| Packaged CMD with a Korean path containing spaces | The three capacity cases repeated with the same request counts and exit codes |

The Ctrl+C test preserves the original interrupted-exec behavior: it does not require a JSON `turn.failed` event that the original interruption path does not emit. An initial fixture with that incorrect expectation was retained and corrected before the passing run. The interruption observation is one timing point, not every completion/cancel race.

## Component and provenance checks

At the same pinned source, the diagnostics executable reported 15 passed tests, no failures and one ignored helper; the TUI library executable reported 5,836 passed tests, no failures and four ignored tests. Packaging-tool tests passed 12 cases using a disposable argv executable. Separate earlier component results are not counted as new runs of this final executable.

History tests include round trips through 50 turns in both directions, release of evicted strong owners checked through Weak references, mutable-source byte accounting, and preservation of the original 100-turn read-only response before display limiting. Display budgets are 30 turns, 400 items and 8 MiB; they are not a total process-memory ceiling. Legacy conversations retain the official full-read fallback and can temporarily load the full history. The model's original history and explicit full exports are separate from display retention.

An exporter test on the actual streaming Drive filesystem killed its owned child during a partial copy, observed no partial final filename, then retried the complete batch with the expected hash. This establishes filesystem publication, not cloud upload acknowledgement.

The public source verifier checks 9,125 imported files, pinned provenance, source resources and 38 Windows prompt input hashes. Its regression suite reported 13 passes and one environment-dependent real-symlink skip. A new full release build from this public checkout is **NOT_RUN**; the private source build above must not be presented as public-checkout reproduction. See [source layout and verification](source-import.md).

## Remaining acceptance

New independent review, local installation evidence, sustained workload measurements and actual authenticated mobile/same-backend verification remain separate gates. Source test counts and a successful release build do not establish that every memory leak, CLOSE_WAIT connection or desktop delay has been resolved. No prompt-cache hit-rate improvement is claimed. Forced termination, early configuration-validation exits and cloud upload acknowledgement are outside the log-preservation guarantee demonstrated here.
