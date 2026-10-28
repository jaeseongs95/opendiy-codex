# CLI validation checkpoint — 2026-10-29

The Windows CLI candidate is `0.165.0+native-r26.20261028`, built from pinned local source `f665ccc64aaf6baee8fd137d8054f1daaae4a398` on official Codex `0.165.0`. Public source export `8571ab5c5ae5b30363e29d8cb9374fba9628c583` was checked separately. This page records actual checks and their limits; it is not a binary release or a complete performance-fix claim.

## Release executable

The locked/offline Windows MSVC release build exited 0 using two Cargo jobs. Release optimization level 3, ThinLTO and four codegen units were retained. The build used a guarded process tree under a separate build directory. After compilation, the source commit/tree, 175 frozen working-input hashes and Cargo.lock were rechecked. The executable SHA-256 was `1c1e24f6735cf186faeec0fa27b7304172f04aad049c533a9a6a4c648a5442fc`.

```sh
cargo build --profile release --offline --locked --target x86_64-pc-windows-msvc -j2 -p codex-cli --bin codex
```

The matching official helper resources were copied into a separate portable package. Its wrapper selects `--no-daemon` to use the patched embedded backend. All 46 packaged file hashes were verified. The official CLI binary was unchanged. No PATH, user authentication, daemon or global configuration was replaced.

## Actual executable checks

These checks ran against the final executable using disposable homes and synthetic model responses, without a real OpenAI account. Model API requests used loopback servers. The initial runs also attempted unauthenticated featured-plugin requests and curated-plugin synchronization on external services; they were not completely offline application tests:

| Boundary | Observed result |
| --- | --- |
| Model capacity, then success | Two requests; one bounded retry; expected acknowledgement; exit 0 |
| Capacity error after partial output | One request; no replay; terminal exit 1 |
| Retry-After beyond retry window | One request; terminal exit 1 |
| Cancel a quiet MCP startup | Cancel ACK and original cancellation response; both exact held Windows process handles signaled before the original request terminated |
| Plugin reload in the actual TUI | ACK identified the target conversation and explicitly distinguished reconnect request from readiness |
| Conversation Fast mode | Priority/default acknowledgements; configuration bytes unchanged across the measured default toggle; no global service tier written |
| Key compatibility | Ctrl+B moved the cursor left; F6 refused when no supported terminal existed |
| Process-local scheduling | Original-conversation registration and repeated loopback execution; pause/list/remove; normal CLI exit |

The TUI used the packaged launcher with read-only sandbox and approval set to never. Initial onboarding wrote its original screen-reader/model-notice preferences before the stable configuration comparison; this was not counted as a Fast-mode write. The reload smoke did not assert that real external MCP servers were ready.

A later test invoked the actual new desktop CMD entry point with inherited Codex/OpenAI environment overrides removed from the child process. Plugins were disabled only in that disposable test configuration. All three capacity scenarios passed again with the same request counts and exit codes. Captured requests reported Originator `codex_exec` and User-Agent `codex_exec/0.165.0+native-r26.20261028`; the earlier runs inherited a Desktop originator override. The new launcher also returned the exact patched version. The existing desktop-app shortcut was preserved.

Prior compiled Windows coverage ran 45 targeted groups with 353 overlapping test executions. That is not 353 unique tests or a full workspace run. Those tests cover additional cleanup, queue/cancel, background-process, keyboard and scheduler races; see [testing methods and exclusions](TESTING.md).

## Public-checkout schema validation

After the release build finished, a new physical checkout of public export `8571ab5` was compiled with the pinned toolchain, locked/offline dependencies and guarded shared build cache:

```sh
cargo test --manifest-path codex-rs/Cargo.toml --locked --offline --target x86_64-pc-windows-msvc --target-dir <build-target> --profile dev-small -j2 -p codex-app-server-protocol --lib schema_fixtures_tests -- --test-threads=1 --nocapture
```

All four read/comparison tests passed; the existing write-fixtures test remained ignored. The compiler's CARGO_MANIFEST_DIR identified the public checkout, rather than a private executable run from a different working directory. Public HEAD, working tree and Cargo.lock stayed unchanged. The generated TypeScript header regression is therefore backed by actual generated comparisons as well as the separate Python guard. No fixture was regenerated to make this test pass.

## Limits and pending acceptance

The designated external reviewer gave a scoped pass for artifact identity, package contents and the public schema correction, with no blocking findings in that scope. Desktop entry-point post-installation review was still pending at this documentation checkpoint. A three-second negative test assertion remains a non-blocking bounded observation, not a general timing guarantee. Raw runtime/build receipts and operational logs remain private. No installable binary or proprietary desktop asset is included here.

The full public CLI release build, Linux checkout, real OpenAI service behavior and sustained user workloads have not been validated by these checks. A short synthetic test cannot establish complete resolution of CLOSE_WAIT growth, memory retention or desktop typing/navigation latency. No prompt-cache hit-rate improvement is claimed.

Korean CLI localization, per-conversation context-window settings, next-turn compaction and a new asynchronous CLI logging pipeline remain follow-up work. The existing official Alt+V clipboard path was preserved rather than reimplemented.
