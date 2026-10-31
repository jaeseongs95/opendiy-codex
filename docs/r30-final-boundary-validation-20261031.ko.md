# R30 최종 경계 검증 소스와 실행 결과 — 2026-11-01

최종 CLI 소스 `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961`의 최적화 빌드와 격리 실행 검사, 이력 시험 및 전체 터미널 화면 시험이 완료됐다. 공개 검증 소스는 PR13으로 `main`의 `9063f288bf13b06a6927b1367d69299c8889c11c`에 반영됐다. 이 문서 갱신은 해당 공개 소스 11개를 보존하며 실제 실행 결과와 그 한계를 기록한다. 공개 저장소를 새로 내려받은 상태에서 빌드하거나 시험한 결과는 아니다.

## 변경과 검증 경계

- 이력 재부착 시험은 정상 `SessionInfoCell` 한 개와 진단 필드가 빈 `StartupWarningsCell` 한 개만 허용한다. 시작 경고는 화면 행이 항상 비므로 모든 진단 필드와 개수를 확인하며, 다른 빈 셀과 실제 오류·경고는 계속 거부한다. 기존 입력·세션·미조회 이력·권한 단언을 유지한다.
- 호스트 통합시험은 실제 V2 자식 작업·로컬 저장소·큐·목표 서비스를 연결해 대기 입력과 미완료 목표 보존, 정확한 선택 범위, 저장소 삭제 중 같은 대상의 변경 직렬화, 연결 종료·요청 취소·시험 장벽 보유자 패닉 뒤 정리 소유권을 검사하도록 작성했다. 임시 홈과 모의 응답 서버를 사용하며 모델 요청 0회를 단언한다. 아래 호스트 7개 시험 결과는 별도 고정 시험 후보에서 얻은 근거다.
- 호스트 시험 모듈과 앱 서버 접근자는 `cfg(all(test, feature = "test-support"))`로 제한된다. 코어 자식 생성 도우미와 로컬 저장소의 일회용 삭제 장벽은 `cfg(feature = "test-support")`로 제한된다. 세 Cargo 정의에는 기본 활성화가 없는 `test-support` 기능만 추가하며 앱 서버 기능이 두 의존 기능을 연결한다. 기존 기본 릴리스의 제품 코드 경로·의존 설정은 변경하지 않는다. `test-support`를 명시적으로 활성화한 빌드는 시험용 장벽과 도우미를 포함하므로 운영 빌드에 사용하지 않는다.

## 출처와 공개 바이트

이력 시험 본문은 고정 소스 `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961`의 한 파일에서 가져왔다. 호스트 통합시험은 기준 `86cf681e4aae35bf0953625784051dea7447d72c`에서 `dd8c677028b3595fab15afba3ae61cb1296a4334`까지의 10개 파일 변경 전체를 함께 가져왔다. 시험 파일만 분리해 필요한 기능 정의와 접근자를 빠뜨리지 않았다.

호스트 대상의 기존 공개 본문은 비공개 기준 본문과 같았다. 다섯 파일에만 기존 공개 수정 고지문 한 줄이 앞에 있었으며 이를 그대로 보존했다. 새로 수정한 파일에는 동일한 공개 고지 관례에 따라 날짜와 이 문서 링크를 덧붙였다. 이력 시험의 기존 고지문도 보존했다. 아래 해시는 정규 Git 소스 바이트를 기준으로 하며, 고지문을 제외한 공개 본문은 해당 고정 소스와 바이트 단위로 일치한다. 기존 `SOURCE_BASELINE.json`과 `upstream/source-inventory.json`은 앞선 제품 스냅샷 기록이고, 이 표는 이번 검증 소스 변경에 대한 추가 대응표다. 전체 실행 파일 재현성을 주장하지 않는다.

| 경로 | 본문 출처 | 원본 SHA-256 | 공개 SHA-256 |
| --- | --- | --- | --- |
| `codex-rs/app-server/Cargo.toml` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `462ba744346d1e96109281defc571bcac539a4670662960defc1f9d9055b2655` | `b0a832e98a6f1ef5ce3f2eb95b2b60510b5984111a43f9be4b686af58bcc48bc` |
| `codex-rs/app-server/src/message_processor.rs` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `9116caf67db9551f7eee2931742581ebb1ccd33e9d194668152763a981b85475` | `ec9034b24761a42328ef791f26c2d3c31c66345a94bc3c0e9fca3f6a631f3674` |
| `codex-rs/app-server/src/message_processor_cleanup_guard_tests.rs` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `e35a8b63211f0da573312f24a7ef1e043ab2dcd8a87b769a0c727254da77b7cd` | `2155d00d727838cdda87d1df4becf9513463297ad6cfe4ccd9f40189d9a00025` |
| `codex-rs/app-server/src/request_processors/thread_delete.rs` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `eb04cf5e594967242797a3075be3f006b0e1bd56421d0b2cd85dc8c7e64498bd` | `04f6fbda0c2e6154716d744dbb2cec62073de4f9cc2c9bf737c441d6f46db32a` |
| `codex-rs/app-server/src/request_processors/thread_goal_processor.rs` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `b0f99f40abb4ab2f079da1a4a1be2238e6b25872fd0d4df7c5e02da7ad3432fb` | `13062e5c80bc11d9bb27c036299d9b0b7578ca3a327589f03ec9f23f7f543498` |
| `codex-rs/core/Cargo.toml` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `03114d3d00e24e19bb85acb86d3a6e5d7774073b711f5bed184a1417cf7e51b4` | `93034d9ac17f2aaf7f1c8027f46581f8591bc18dd9405b467e5ff646b6417373` |
| `codex-rs/core/src/thread_manager.rs` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `4dfd54a2b3c2e90416bf6281f310508735c612c88297fe0ba97d2b6d44af643c` | `559e51c73fe26dc664098e5c62f46389917a7b0a7c5c84f468b759f28823d3cd` |
| `codex-rs/thread-store/Cargo.toml` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `a615d225fb63b8d998f07e6bb485776e07ad6d18c97c9f0081274c6cc507da16` | `bee4145770dff682a7d4478e1abe963b8ae2e670e05eba7ea913440dbb8f7d29` |
| `codex-rs/thread-store/src/local/delete_thread.rs` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `02c3ef8bdf03f2580e01c78efce9b65079b91a4a7241985ae7f09f79b062c30a` | `5aa379430f8891e4c24582a28d96ddfd6749c494b1d23936135288a7df319b60` |
| `codex-rs/thread-store/src/local/mod.rs` | `dd8c677028b3595fab15afba3ae61cb1296a4334` | `c4172221147d0b12ea23b7b7c74df20008cfb4de50fb1d99a364739a8f0e3c39` | `c06f0bb6971336389c4b30606e2395c3c9bb24d3a9c5c10a4ab131235f6613b9` |
| `codex-rs/tui/src/app/tests/history_hydration_tests.rs` | `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961` | `5568043a4fba487a678be428c521b0b859e8541cfeb377441d71ce02af918186` | `23d00277d00b1b7eef484c1c68c0013c85e724dce5661477585c000ec3240825` |

## 실제 검증 결과

아래 기록은 격리된 비공개 검증 환경의 합성 시험·빌드·포장 원자료를 대조한 결과다. 원자료와 실행 파일은 이 문서 변경에 포함하지 않는다. 통과 수는 실행별로 표시하며 서로 더하지 않는다.

| 검사 | 대상 소스 | 실제 결과 | 원자료 식별자 |
| --- | --- | --- | --- |
| UI05 수정 전 이력 경계 | `31082c2f999b0e26eb8eb34feb71658c505d585e` | 컴파일 종료 0 뒤 시험 종료 101, 0 통과·1 실패. 실제 미저장 이력 오류 셀 하나로 실패하며 정상 빈 시작 경고 셀은 실패 원인에서 제외 | `r30-history-ui-red-green-05/red-live-01/results.json` |
| UI05 수정 후 이력 시험 | `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961` | 컴파일·시험 종료 0, 8 통과·0 실패·0 제외 | `r30-history-ui-red-green-05/green-history-ui-01/results.json` |
| 전체 터미널 화면 시험 | `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961` | 컴파일·시험 종료 0, 5,860 통과·0 실패·4 제외 | `r30-history-ui-final-wide-05/results.json` |
| 호스트 정리 경계 통합시험 | `dd8c677028b3595fab15afba3ae61cb1296a4334` | 컴파일·시험 종료 0, 7 통과·0 실패·0 제외 | `r30-host-integration-run-03/results.json` |
| 최종 CLI 최적화 빌드 | `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961` | 2026-10-31 종료 0, `x86_64-pc-windows-msvc`, `--profile release --offline --locked -j1` | `r30-release-build-02/receipt.json` 및 `r30-release-followup-02/build-finished.json` |
| 포장에 사용한 앱 서버 최적화 빌드 | `86cf681e4aae35bf0953625784051dea7447d72c` | 2026-10-30 종료 0. 기존 앱 서버 실행 파일 재사용 | `r30-app-server-release-build-01/receipt.json` |
| 최종 CLI 실제 실행 3건 | 아래 CLI 실행 파일 해시 | 검사기 종료 0. 개별 종료는 순서대로 0·1·1, 요청 수 2·1·1 | `runtime-tests/candidate-r30-final-02/summary.json` 및 `r30-release-followup-02/finished.json`의 `smokeExitCode` |
| 별도 포장·압축 무결성 | 아래 CLI·앱 서버 실행 파일 | 포장 재실행 종료 0 | `r30-package-result-02.json` |

실제 CLI 실행은 재시도 후 성공, 일부 출력 뒤 재전송 금지, 허용 재시도 시간을 넘는 `Retry-After`의 종료 경계를 검사했다. 뒤의 두 개별 종료 1은 각 사례가 기대한 오류 종료다. 세 실행은 모두 같은 최종 CLI 해시를 사용했고 시간 초과가 없었으며, 환경을 정리하고 시험용 플러그인 기능을 비활성화했다. 기록 수집은 활성화된 상태였다.

후속 실행기의 원래 `finished.json`에는 전체 종료 1, 빌드 종료 0, 실제 실행 검사 종료 0, 포장 종료 1이 각각 남아 있다. 포장기가 자신이 끝난 뒤 생성될 `finished.json`을 먼저 읽어 파일 부재로 실패한 결과다. 포장 입력을 이미 완료된 `build-finished.json`으로 바꾼 재실행은 종료 0이며, 원래 실패 기록은 보존했다. 이를 제품 빌드·시험 실패나 원래 후속 실행기 전체의 종료 0으로 바꾸어 기록하지 않는다.

## 실행 파일과 제품 소스 대응

| 산출물 | 빌드 소스 | SHA-256 |
| --- | --- | --- |
| `codex.exe` | `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961` | `73b4bef303e23792853392859ea0f724dd3136348458a2c32a9a40c8845ffe6a` |
| `codex-app-server.exe` | `86cf681e4aae35bf0953625784051dea7447d72c` | `f6148551bc2e5d4a25358bcfd7f889f5c1c2b9bd777d62ae93600ae87a9556d0` |
| `opendiy-codex-r30-windows-x86_64.zip` | 위 두 실행 파일의 별도 포장 | `aca3da2f3d8c3afd1e1c737d04a00ddb16d7fb9aba567702964fa2ac5fd08cc9` |

CLI 버전 확인 결과는 `codex-cli 0.165.0+native-r30.20261030`이다. CLI와 앱 서버의 빌드 소스는 서로 다르다. `86cf681e`에서 최종 CLI 소스 `1cacc86a`까지의 변경 12개 파일은 모두 `codex-rs/tui` 아래에 있으며, 이 포장은 앱 서버를 최종 CLI 소스에서 새로 빌드했다고 주장하지 않는다.

공개 제품 스냅샷의 `SOURCE_BASELINE.json`은 `57957241e90439377f4e34afbec79a6422f5872c`를 가리킨다. 이 소스와 최종 CLI 소스 `1cacc86a` 사이의 변경은 위 이력 시험 한 파일뿐이다. 공개 PR13은 그 시험과 호스트 시험에 필요한 10개 파일만 추가로 반영했다. 따라서 공개 수정 고지문과 앞 표의 시험 전용 차이를 구분하면 기본 릴리스의 제품 코드 경로는 유지된다. 호스트 시험 도우미는 기본 비활성 `test-support`에 한정되며, 이 판단은 해당 기능을 켠 운영 빌드의 동등성을 포함하지 않는다. 호스트 7개 통과는 `dd8c6770`의 시험 실행 근거이며 최종 CLI 소스 전체에서 다시 실행한 기록으로 취급하지 않는다.

## 확인 범위와 남은 한계

공개 문서 갱신에서는 변경 범위, 기존 소스 11개와 출처 대응표 보존, 원자료별 소스·종료 코드·통과 수, UTF-8과 줄바꿈 및 공백을 확인했다. 실행 파일·원시 로그·개인 절대 경로·인증 자료는 포함하지 않았다. 서버 설치와 후속 미완료 기능도 이 변경에 포함하지 않았다.

기존 시험 실행 방법을 사용할 때 이력 시험은 `cargo test -p codex-tui --lib live_unmaterialized_resume_keeps_session_input_and_incomplete_history -- --test-threads=1`, 호스트 시험은 `cargo test -p codex-app-server --features test-support --lib cleanup_guard_tests -- --test-threads=1`로 선택할 수 있다. 이 명령은 재현 방법이며 이번 공개 문서 작업에서 실행하지 않았다.

이 결과는 전체 작업공간의 모든 시험, 실제 데스크톱 앱에서의 장시간 부하와 모든 사용자 흐름, 공개 사본에서의 새 빌드와 실행 파일 재현성을 증명하지 않는다. 독립감사·설치·공개 릴리스·예약 작업 중지는 별도 단계이며 이 문서의 통과 결과로 완료를 선언하지 않는다.
