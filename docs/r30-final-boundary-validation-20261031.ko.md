# R30 최종 경계 검증 소스 — 2026-10-31

이 변경은 공개 기준 `e0962cd0360e6e017588e46821560c09db1d60c7`에 검증 소스만 추가한 작업 후보다. 제품 수정과 실행 결과의 기준은 기존 고정 소스 기록을 따른다. 별도 격리 환경의 UI05에서 제품 수정 전 후보 `31082c2f999b0e26eb8eb34feb71658c505d585e`는 컴파일 종료 0 뒤 같은 이력 시험이 실제 미저장 이력 오류 셀 하나로 실패했다(종료 101, 0 통과·1 실패). 정상 빈 시작 경고 셀은 더 이상 실패 원인에 섞이지 않았다. 수정 후 후보 `1cacc86a1d2ef28d4dd79ef7b2f1334387a12961`의 검증과 후속 CLI release02 파이프라인은 진행 중이며, 이 공개 사본의 컴파일·시험·제품 빌드는 실행하지 않았다. 이전 후보의 통과 수를 이 후보의 통과 근거로 사용하지 않는다.

## 변경과 검증 경계

- 이력 재부착 시험은 정상 `SessionInfoCell` 한 개와 진단 필드가 빈 `StartupWarningsCell` 한 개만 허용한다. 시작 경고는 화면 행이 항상 비므로 모든 진단 필드와 개수를 확인하며, 다른 빈 셀과 실제 오류·경고는 계속 거부한다. 기존 입력·세션·미조회 이력·권한 단언을 유지한다.
- 호스트 통합시험은 실제 V2 자식 작업·로컬 저장소·큐·목표 서비스를 연결해 대기 입력과 미완료 목표 보존, 정확한 선택 범위, 저장소 삭제 중 같은 대상의 변경 직렬화, 연결 종료·요청 취소·시험 장벽 보유자 패닉 뒤 정리 소유권을 검사하도록 작성했다. 임시 홈과 모의 응답 서버를 사용하며 모델 요청 0회를 단언한다. 시험 소스의 존재를 이 경계의 통과로 해석하지 않는다.
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

## 현재 확인과 남은 범위

이번 공개 작업의 확인 범위는 변경 범위, 고지문 보존, 본문 바이트 대응, 시험 조건부 경계, 단일 파일 형식과 공백 검사다. 실행 파일·원시 로그·개인 절대 경로·인증 자료는 포함하지 않는다. 서버 설치와 후속 미완료 기능은 포함하지 않았다.

기존 시험 실행 방법을 사용할 때 이력 시험은 `cargo test -p codex-tui --lib live_unmaterialized_resume_keeps_session_input_and_incomplete_history -- --test-threads=1`, 호스트 시험은 `cargo test -p codex-app-server --features test-support --lib cleanup_guard_tests -- --test-threads=1`로 선택할 수 있다. 이 명령들은 재현 방법이며 이 공개 후보에서 실행한 기록이 아니다. 최종 컴파일·동일 시험 바이트의 제품 수정 전후 비교·전체 회귀·최적화 빌드·실제 앱 부하는 별도 결과가 필요하다.
