# Official 0.165.0 source snapshot

The current snapshot is official `rust-v0.165.0` (`360319d8dee62d87e2d59fc0c21805d79519cfed`) plus the local source revision `67404f486047863ff7e56d7f47f4c350a872b483`. It is identified by [SOURCE_BASELINE.json](../SOURCE_BASELINE.json), with file hashes in `upstream/source-inventory.json` and changes in `upstream/official-0.165.0.patch`. The English and Korean READMEs describe R28; root LICENSE/NOTICE are unchanged. [R28 validation](cli-r28-validation.md) distinguishes the local build from the unbuilt public export. See [MODIFICATIONS.md](../MODIFICATIONS.md) for the current change notice.

이번 고정 소스에는 [자연어 예약 도구 연결과 사용 안내](native-cli-cron.ko.md)가 포함됩니다. 이 변경은 기존 R28 실행 파일에 설치되지 않았고 검증 중입니다. 실제 모델의 자연어 일정 해석과 도구 선택 평가는 실행하지 않았습니다. 아래 R28 빌드·시험 기록은 이전 소스의 결과이며 이 후속 후보의 통과 근거가 아닙니다.

새 고정 소스 `67404f486047863ff7e56d7f47f4c350a872b483`는 `004c9113ff02c292998f1fa03355afe3dfe6d404` 이후 시험 3파일만 보강한 후보다. 제품 코드는 동일하다. 직전 `4f`의 화면·계약 시험은 통과했지만 전체 TUI 시험에서 1개가 실패했으며 시험 안내에 기록했다. 직전 `92d`의 06 TUI 컴파일은 시험 준비 호출의 `anyhow::Error`를 `eyre`로 자동 변환하지 못한 `E0277`로 종료 코드 101이었고 실행시험·일반 실행 파일 빌드는 미실행이다. `704`는 그 호출 한 곳을 인접 시험과 같은 명시적 준비 실패 `expect`로 바꿨다. `704`의 07 시험 컴파일은 통과했지만 대화 선택 시험은 1개 통과·2개 실패였고 일반 개발 빌드는 미실행이다. 이번 `674`는 설정 변경 알림과 `includeTurns=false` 생략 계약을 시험에 반영한 수정이다. 08 일반 개발 CLI·TUI 시험 컴파일과 영향 시험 66개, 고정 개발 실행 파일의 격리 시험 두 실행기는 종료 코드 0이었다. 전체 TUI를 새로 전량 통과한 결과는 아니다. 독립감사 08은 고정 소스·개발 실행 파일·66개 영향 시험·격리 실행에 한정해 PASS였고, 실제 Desktop 공유 실행 1회의 별도 확인 범위와 모바일·장시간 부하·최적화 release·설치 미완료는 시험 안내에 구분했다. 이전 `9766` 후보의 표적 107개·전체 TUI 5,849개 통과와 고정 개발 실행 파일·감사 07 결과는 이번 후보의 통과 근거가 아니다. [후속 시험 방법과 범위](TESTING.md#빈-미저장-대화-이력-복원-후속-후보)를 참고한다.

이전 자연어 예약 후보 `921d404013da2035f90e9ab055acadc957a88592`의 전체 TUI 시험은 5,845개 통과·4개 실패·4개 건너뜀으로 종료 코드 101이었다. 실패 중 2개는 MCP 요청의 필수 메타데이터 누락으로 `a3edca405d1eddf138f549764459394603576804`에서 보강했다. 나머지 2개는 새 `cron` 노출을 빠뜨린 도구 목록·`deferLoading` 기대값으로 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609`에서 보강했다.

직전 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609` 후보의 표적 실행은 100개 통과·7개 실패였고, 8개 실행 그룹 중 3개 통과·5개 실패였다. 이전 `9766a8b7b8c4d56d7076367ddc12f2917a3df197` 후보에서는 표적 107개 통과·0개 실패와 전체 TUI 5,849개 통과·0개 실패·4개 무시를 확인했다. 이전 후보별 실패와 통과 기록은 새 후보의 진행 상황과 구분한다.

현재 후보의 제품 변경과 시험 보강 범위는 [시험 안내](TESTING.md#빈-미저장-대화-이력-복원-후속-후보)에 정리했다.

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
