# 공식 0.166.0 기반 R31 소스

현재 고정 후보는 `7f629944e3b2808093853f0bdeef2218fc240ae3`이며 공식 `rust-v0.166.0` 전체를 기반으로 한다. 이전 공개 커밋 `f28920f`의 후속 시험도 보존했다. [출처 목록](../SOURCE_BASELINE.json)·`upstream/source-inventory.json`·`upstream/official-0.166.0.patch`가 현재 입력을 식별한다.

소스 내보내기 검사는 통과했으나 제품 컴파일·회귀 검증은 진행 중이다. 현재 실행·설치된 R30과 구분하며, [변경 및 검증 범위](../MODIFICATIONS.md)와 [시험 안내](TESTING.md)를 따른다. 기존 README·라이선스·고지문은 보존했다.

## 이전 소스 스냅샷 이력

아래의 버전·검증·설치 설명은 해당 후보를 기록한 당시의 상태다.

# Official 0.165.0 source snapshot

The current snapshot is official `rust-v0.165.0` (`360319d8dee62d87e2d59fc0c21805d79519cfed`) plus the local source revision `57957241e90439377f4e34afbec79a6422f5872c`. It is identified by [SOURCE_BASELINE.json](../SOURCE_BASELINE.json), with file hashes in `upstream/source-inventory.json` and changes in `upstream/official-0.165.0.patch`. The English and Korean READMEs identify the R30 candidate; root LICENSE/NOTICE are unchanged. [R30 검증 상태](TESTING.md)는 현재 소스의 시험 근거와 미완료 범위를 구분하며, [R28 검증](cli-r28-validation.md)은 이전 빌드의 이력이다. See [MODIFICATIONS.md](../MODIFICATIONS.md) for the current change notice.

R30 후속 고정 소스 `57957241e90439377f4e34afbec79a6422f5872c`는 공식 0.165.0 기반 R29·R30 위에 TUI 이력 표시 수정을 더한 후보이며, 버전은 `0.165.0+native-r30.20261030`이다. 공식 0.166 전체 기반으로 전환한 소스가 아니다. 이전 `86cf681e4` 대비 TUI 12파일이 바뀌었고, 공식 0.165.0 대비 수정 목록은 451파일이다. 성공한 대화 재개 뒤 아직 저장되지 않은 이력을 실패나 빈 완료로 확정하지 않도록 `Deferred` 상태와 저장 대기 안내를 분리했다. 불완전 이력·활성 입력의 보호를 유지하고, 정상 저장 대기에는 오류 셀이나 근거 없는 재시도 안내를 만들지 않도록 소스를 수정했다. 일반 오류와 `unknown`·`deleted` 사유는 기존 오류 경로로 남긴다. 새 안내는 기존 `i18n::text`와 사전을 사용해 기본 영어와 전역 한국어 선택에 연동한다. 지정 커밋 이후의 변경과 실시간 작업트리의 미커밋 변경은 포함하지 않는다.

이번 후보에는 시험 전용 `089257a53`의 이력 복원 회귀시험, `7a7ed5fa`의 제품 수정, `57957241`의 언어 연동과 실제 footer·검색 안내 소비 회귀시험 소스가 포함된다. 영어·한국어 선택별 일반·좁은 화면 안내와 검색 원문 미번역을 단언하는 시험을 추가했다. 이 후보의 Cargo 컴파일·회귀시험과 새 최적화 제품 빌드·실행 검증은 미실행이다. 시험 소스 추가를 시험 통과로 해석하지 않으며, 이전 `86cf681e4`의 6,307회 및 추가 시험 결과를 새 후보 전체의 통과 근거로 사용하지 않는다. 공개 내보내기 검증은 고정 Git 객체와 파일·프롬프트 바이트의 대응, 허용 범위와 민감정보 검사에 한정한다.

### 이전 86cf681e4 검증 이력

아래의 실행 파일·시험 결과와 완료·미완료 상태는 이전 `86cf681e4` 후보를 문서화한 시점의 기록이다. 현재 후보의 빌드·시험·설치 근거가 아니다.

이전 고정 소스 `86cf681e4`의 `compile13`과 후속 검증·빌드 단계는 종료 코드 0이었다. 같은 소스의 초기 5그룹과 후속 10그룹 결과 파일에는 합계 15그룹·6,307회 통과·0회 실패·5회 무시가 기록됐고, 각 그룹의 종료 코드는 0이었다. 이 가운데 전체 TUI 시험은 5,859개 통과·4개 무시, 전체 CLI 시험은 334개 통과·1개 무시였다. 합계는 기록된 실행 횟수이며, 필터 겹침을 제거한 고유 시험 수나 전체 작업공간의 모든 시험을 통과했다는 뜻은 아니다.

같은 고정 소스의 최적화 CLI 빌드는 종료 코드 0이었다. 최종 CLI 식별 자료와 검증 영수증의 버전은 `codex-cli 0.165.0+native-r30.20261030`, 실행 파일 SHA-256은 `1954507fdaaf0ecdd568725853b0735d20d426cbf97cb09bcd6ac5f592ddef21`로 일치하며, 검증 영수증의 종료 코드는 0이었다. 이 SHA의 CLI를 사용한 격리 실행 요약에서는 `retry`가 요청 2회·종료 0, `partial-no-replay`와 `hint-exceeds-window`가 각각 요청 1회·종료 1이었고 시간 초과는 없었다. 뒤의 두 종료 1은 요약에 명시된 오류 종료 경계의 관측값이며, 추가 응답 내용이나 전체 기능 통과를 뜻하지 않는다.

앱 서버의 최적화 실행 파일은 별도로 빌드 중이며 아직 완료·설치되지 않았다. CLI와 앱 서버를 합친 최종 독립감사·설치·공개 Release·실제 앱의 장시간 부하·모바일 검증은 완료 근거가 없다. 공개 소스 사본에서 새로 빌드하거나 실행한 결과가 아니며, 원시 시험 로그·실행 파일·개인 경로·인증 정보는 공개하지 않는다. 이전 후보의 부분 실패와 보호 중단은 [시험 이력](TESTING.md)에 구분해 남긴다.

이전 R30 부분시험 이력(각 후보 당시의 상태): `1253c2014`의 통합 시험 컴파일은 TUI 시험의 비공개 필드 접근 2건으로 종료 코드 101이었다. 같은 컴파일에서 생성된 실행 파일로 스키마 시험 4개 통과·0개 실패·1개 무시와 CLI 정리 인자 시험 1개 통과·0개 실패를 확인했다. `bc1db4b14`는 해당 TUI 시험을 공개 입력 경계로 바꾼 후속 수정이며 이 소스 갱신 시점에는 재컴파일 전이다. `6855f4fd3`의 전역 언어 변경, `f120854c8`의 MCP 정리 경계 시험, 정리 수명 동적 시험, 후속 공동 입력 인터페이스·최종 독립감사·설치·모바일 검증은 미완료다. 앞선 R29 결과를 R30의 통과 근거로 재사용하지 않으며, 공개 사본의 제품 빌드·실행 결과를 주장하지 않는다.

다음 문단은 이전 R29 소스 갱신 시점의 이력이다.

R29 고정 소스 `af098cef617d62fdfce51d5e1fbd0afb4533f75d`는 공식 0.165.0 기반에 공식 0.166 변경 5개만 선별 이식한 후보다. 전체 0.166 기반으로 전환하지 않았으며 버전 표기는 `0.165.0+native-r29.20261030`이다. 이전 `67404f4` 이후 34파일이 바뀌었고 공식 0.165.0 대비 수정 목록은 394파일이다. 제품 로직의 마지막 변경은 `159017bd9`이며, 이후에는 `3fdd3c3`의 Windows 알림 시험과 `af098ce`의 버전 표기만 바뀌었다. `159017bd9`에서 단위시험 11그룹 802개 통과·0개 실패·프로토콜 무시 1개와 표적 통합시험 5개 통과·0개 실패를 확인했다. `3fdd3c3`에서 Windows 알림 경계 시험 1개 통과·0개 실패·무시 0개와 실행 전후 원본 입력 불변을 확인했다. 최종 `af098ce`에서는 버전 표기 168곳 치환과 오프라인 잠금 메타데이터 검사 종료 코드 0을 확인했다. 이 결과는 808개 시험을 최종 커밋에서 전량 재실행했다는 뜻이 아니다. 이번 소스 갱신 시점에는 최적화 빌드가 진행 중이며, 새 실행 파일의 검증·제품 독립감사·설치는 미완료다. 공개 사본의 새 빌드·실행 결과나 실제 캐시율·장기 부하 개선 측정 결과는 아니다.

이전 고정 소스 `67404f4`에 포함된 [자연어 예약 도구 연결과 사용 안내](native-cli-cron.ko.md)의 설치 기록은 아래에 보존합니다. 새 R29 후보의 설치·실행 결과가 아닙니다. 기존 R28 실행 파일은 보존하고 사용자 승인 후 새 패키지의 명령줄 바로가기·기본 공유 서버로 전환했습니다. 기능 전체의 검증 완료를 뜻하지는 않습니다. 실제 모델의 자연어 일정 해석과 도구 선택 평가는 실행하지 않았습니다. 아래 R28 빌드·시험 기록은 이전 소스의 결과이며 이 후속 후보의 통과 근거가 아닙니다.

이전 고정 소스 `67404f486047863ff7e56d7f47f4c350a872b483`는 `004c9113ff02c292998f1fa03355afe3dfe6d404` 이후 시험 3파일만 보강한 후보다. 제품 코드는 동일하다. 직전 `4f`의 화면·계약 시험은 통과했지만 전체 TUI 시험에서 1개가 실패했으며 시험 안내에 기록했다. 직전 `92d`의 06 TUI 컴파일은 시험 준비 호출의 `anyhow::Error`를 `eyre`로 자동 변환하지 못한 `E0277`로 종료 코드 101이었고 실행시험·일반 실행 파일 빌드는 미실행이다. `704`는 그 호출 한 곳을 인접 시험과 같은 명시적 준비 실패 `expect`로 바꿨다. `704`의 07 시험 컴파일은 통과했지만 대화 선택 시험은 1개 통과·2개 실패였고 일반 개발 빌드는 미실행이다. 이번 `674`는 설정 변경 알림과 `includeTurns=false` 생략 계약을 시험에 반영한 수정이다. 08 일반 개발 CLI·TUI 시험 컴파일과 영향 시험 66개, 고정 개발 실행 파일의 격리 시험 두 실행기는 종료 코드 0이었다. 전체 TUI를 새로 전량 통과한 결과는 아니다. 독립감사 08은 고정 소스·개발 실행 파일·66개 영향 시험·격리 실행에 한정해 PASS였다. 이후 최적화 CLI·별도 패키지 검사와 실제 Desktop 공유 실행도 확인했고 독립감사 09도 별도 패키지·최적화 격리 실행·공유 8검사 범위에서 `PASS(한정)`였다. 사용자 승인 후 새 명령줄 바로가기·기본 공유 서버를 전환했고 사후감사 11은 그 범위에서 `PASS(한정)`였다. 모바일·장시간 부하는 미완료다. 범위는 시험 안내에 구분했다. 이전 `9766` 후보의 표적 107개·전체 TUI 5,849개 통과와 고정 개발 실행 파일·감사 07 결과는 이번 후보의 통과 근거가 아니다. [후속 시험 방법과 범위](TESTING.md#빈-미저장-대화-이력-복원-후속-후보)를 참고한다.

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
