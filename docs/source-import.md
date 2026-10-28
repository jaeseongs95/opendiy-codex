# Official 0.165.0 source snapshot

The current snapshot is official `rust-v0.165.0` (`360319d8dee62d87e2d59fc0c21805d79519cfed`) plus the local source revision `f665ccc64aaf6baee8fd137d8054f1daaae4a398`. It is identified by [SOURCE_BASELINE.json](../SOURCE_BASELINE.json), with file hashes in `upstream/source-inventory.json` and changes in `upstream/official-0.165.0.patch`. The existing project README prose is preserved with one current-source link added in each language; root LICENSE/NOTICE are unchanged. README's r22 scope is historical. See [MODIFICATIONS.md](../MODIFICATIONS.md) for the current change notice.

The selected public Git paths are `codex-rs`, `third_party`, `patches`, `.cargo/config.toml` and `tools/windows-cli`, plus the root `.gitattributes`. Existing public attributes are preserved before a managed block containing the pinned source's generated-file and LF rules; `checks/.gitattributes` is excluded. The full Rust workspace and its local path dependencies are retained: exporting just CLI/app-server would omit shared build inputs. Compressed schema resources, Rust include files, source fixtures and vendored license files are build inputs, not installable binaries. Upstream automation within the selected source is not enabled. No local `checks`, logs, profiles, caches, target directories, credentials, receipts, private desktop assets or source-repository Git history are copied.

The main entry points are `codex-rs/cli` and `codex-rs/app-server`. Shared implementations include `core`, `codex-mcp`, `rmcp-client` and `utils/pty`. Cargo.lock keeps dependency provenance, including registry `mio 1.2.4`; no machine-specific dependency path override is added. Follow `codex-rs/rust-toolchain.toml` and install the matching platform build tools. Network dependency downloads require a separately prepared build environment.

The inventory's `sourceSha256` is the exact pinned Git blob; `publishedSha256` is canonical published Git bytes after notices and the attributes merge. This is distinct from physical Windows working files. The 38 recorded embedded prompt paths alone use native `text` EOL rules, overriding the repository's old `* -text` default. With Windows Git `core.autocrlf=true` and `core.eol=native`, actual materialization matches the pinned Windows source hashes in `upstream/windows-prompt-inputs.json`; compressed schema resources remain byte-exact. No `eol=crlf` rule is imposed on Linux. The exporter materializes and the verifier checks those raw Windows prompt hashes, then normalizes only these 38 files for canonical Git hash comparison. A Windows checkout using incompatible EOL overrides must pass this check before building. This input-byte check covers the recorded prompts, not a reproduced public binary or all platform-specific build inputs.

```sh
python -B scripts/source-import/verify.py
python -B scripts/source-import/test_verify.py
cd codex-rs
cargo metadata --no-deps --locked --offline --format-version 1
cargo build --locked -p codex-cli --bin codex
cargo build --locked -p codex-app-server --bin codex-app-server
```

Offline metadata requires dependencies already present. These are reproducible commands, not a claim that this public checkout's complete build or runtime tests have been executed. [TESTING.md](TESTING.md) gives targeted real-boundary test commands and limitations. The Windows packaging helper and disposable argv test are documented in [tools/windows-cli/README.md](../tools/windows-cli/README.md). Building or packaging does not install the result; generated binaries/packages/receipts should remain outside this source checkout.

The exporter reads pinned committed Git blobs from a separately supplied repository, never machine working files. In a separate clean worktree containing the previous verified import, update with:

```sh
python -B scripts/source-import/export.py --source-repository <source-repository> --update
```

The update verifies all previous inventoried file bytes and all output collisions before writing. Changed prior imports or unrelated colliding files cause failure. It retires only verified inventoried files missing from the new snapshot and leaves project documents and historical r22 provenance in place. Without `--update`, different existing bytes are never overwritten. The source repository's Git history is neither copied nor merged.

`codex-rs/vendor/bubblewrap/LICENSE` is the original Git symlink to its adjacent `COPYING`. Windows without symlink support stores the target string as a placeholder. After reviewing and staging the source, run `python -B scripts/source-import/export.py --stage-modes` to restore inventoried symlink/executable Git modes. The verifier checks both an actual LICENSE symlink and its Windows placeholder. Existing third-party licensing remains unchanged; no third-party material is relicensed as Apache-2.0.
