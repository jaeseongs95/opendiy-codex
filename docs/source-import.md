# Official 0.165.0 source snapshot

The current snapshot is official `rust-v0.165.0` (`360319d8dee62d87e2d59fc0c21805d79519cfed`) plus the local source revision `9766a8b7b8c4d56d7076367ddc12f2917a3df197`. It is identified by [SOURCE_BASELINE.json](../SOURCE_BASELINE.json), with file hashes in `upstream/source-inventory.json` and changes in `upstream/official-0.165.0.patch`. The English and Korean READMEs describe R28; root LICENSE/NOTICE are unchanged. [R28 validation](cli-r28-validation.md) distinguishes the local build from the unbuilt public export. See [MODIFICATIONS.md](../MODIFICATIONS.md) for the current change notice.

이번 고정 소스에는 [자연어 예약 도구 연결과 사용 안내](native-cli-cron.ko.md)가 포함됩니다. 이 변경은 기존 R28 실행 파일에 설치되지 않았고 검증 중입니다. 실제 모델의 자연어 일정 해석과 도구 선택 평가는 실행하지 않았습니다. 아래 R28 빌드·시험 기록은 이전 소스의 결과이며 이 후속 후보의 통과 근거가 아닙니다.

이번 후보에는 로드된 미저장 로컬 대화의 `excludeTurns` 재접속과 기본 `app-server proxy`의 Windows 비공개 경로·연결 상대 검사도 포함된다. 프록시 회귀 3개 함수의 3사례와 재개 회귀 4개 함수의 10사례를 포함해 표적 107개 시험이 통과했다. 별도의 전체 TUI 시험은 5,849개 통과·0개 실패·4개 무시였다. 고정 원본 소스의 표적 시험·전체 TUI 시험과 일반 개발 실행 파일의 제한된 격리 시험을 완료했다. 독립감사 07의 통과는 캐시 복구와 해당 개발 실행 파일의 격리 실행 범위에 한정되며, 전체 제품 완료·설치 가능 상태는 `BLOCKED`다. [변경 고지](../MODIFICATIONS.md)와 [실행한 시험 범위](TESTING.md#공유-로컬-서버-후속-회귀시험검증-범위)를 참고한다.

이전 자연어 예약 후보 `921d404013da2035f90e9ab055acadc957a88592`의 전체 TUI 시험은 5,845개 통과·4개 실패·4개 건너뜀으로 종료 코드 101이었다. 실패 중 2개는 MCP 요청의 필수 메타데이터 누락으로 `a3edca405d1eddf138f549764459394603576804`에서 보강했다. 나머지 2개는 새 `cron` 노출을 빠뜨린 도구 목록·`deferLoading` 기대값으로 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609`에서 보강했다.

직전 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609` 후보의 표적 실행은 100개 통과·7개 실패였고, 8개 실행 그룹 중 3개 통과·5개 실패였다. 이번 `9766a8b7b8c4d56d7076367ddc12f2917a3df197` 후보에서는 표적 107개 통과·0개 실패와 전체 TUI 5,849개 통과·0개 실패·4개 무시를 확인했다. 앞선 후보의 실패 기록도 보존하며 현재 결과와 구분한다.

이번 제품 변경과 시험 보강 범위는 [시험 안내](TESTING.md#공유-로컬-서버-후속-회귀시험검증-범위)에 정리했다.

The selected public Git paths are `codex-rs`, `third_party`, `patches`, `.cargo/config.toml` and `tools/windows-cli`, `docs/native-cli-r27.ko.md`, `docs/native-cli-cron.ko.md`, plus the root `.gitattributes`. Existing public attributes are preserved before a managed block containing the pinned source's generated-file and LF rules; `checks/.gitattributes` is excluded. The full Rust workspace and its local path dependencies are retained: exporting just CLI/app-server would omit shared build inputs. Compressed schema resources, Rust include files, source fixtures and vendored license files are build inputs, not installable binaries. Upstream automation within the selected source is not enabled. No local `checks`, logs, profiles, caches, target directories, credentials, receipts, private desktop assets or source-repository Git history are copied.

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

Offline metadata requires dependencies already present. Build and test commands use `--locked`. Cargo.lock is generated input and Cargo can regenerate its comments when intentionally rewriting it; if its bytes change, the source verifier will no longer accept the frozen export. Review that change or re-export from the pinned source instead of treating the modified lockfile as the original publication. These are reproducible commands, not a claim that this public checkout's complete build or runtime tests have been executed. [TESTING.md](TESTING.md) gives targeted real-boundary test commands and limitations. The Windows packaging helper and disposable argv test are documented in [tools/windows-cli/README.md](../tools/windows-cli/README.md). Building or packaging does not install the result; generated binaries/packages/receipts should remain outside this source checkout.

The exporter reads pinned committed Git blobs from a separately supplied repository, never machine working files. In a separate clean worktree containing the previous verified import, update with:

```sh
python -B scripts/source-import/export.py --source-repository <source-repository> --update
```

The update verifies all previous inventoried file bytes and all output collisions before writing. Changed prior imports or unrelated colliding files cause failure. It retires only verified inventoried files missing from the new snapshot and leaves project documents and historical r22 provenance in place. Without `--update`, different existing bytes are never overwritten. The source repository's Git history is neither copied nor merged.

`codex-rs/vendor/bubblewrap/LICENSE` is the original Git symlink to its adjacent `COPYING`. Windows without symlink support stores the target string as a placeholder. After reviewing and staging the source, run `python -B scripts/source-import/export.py --stage-modes` to restore inventoried symlink/executable Git modes. The verifier checks both an actual LICENSE symlink and its Windows placeholder. Existing third-party licensing remains unchanged; no third-party material is relicensed as Apache-2.0.
