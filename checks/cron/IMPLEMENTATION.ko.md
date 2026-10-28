# CLI cron 구현 단위

기준은 `feature/cli-cron`, `7673c44501be6b9172b662c67ac2e319a381e493`이다. 설치·배포·공식 실행 파일·daemon·인증·전역 설정은 바꾸지 않았다. 이 기록은 구현 PASS나 독립감사 결과가 아니다.

`/cron add --every 10m -- <prompt>`, `add --at <RFC3339+offset> -- <prompt>`, `list`, `pause/resume/remove/cancel <id>`를 연결했다. CLI 수명 안의 메모리 예약이며 새 DB·OS scheduler·job별 polling thread가 없다. event loop의 가장 가까운 deadline 하나를 사용한다. 지연·절전 뒤에는 admission을 최대 한 번만 시도하고 다음 due를 `now+interval`로 잡는다. busy면 그 한 회도 skip하며 일회성 busy는 종료 상태다.

예약에는 backend/epoch/thread와 등록 시 prompt를 보관한다. 현재 화면의 모델·권한·cwd·draft·이미지를 실행 시 차용하지 않는다. 새 `turn/startIfIdle`은 일반 `turn/start`와 같은 이미지·입력 크기·직접 입력 소유권·모델 provider 검사를 거쳐 core의 원자 `start_turn_if_idle`로 들어간다. 일반 start fallback은 없다. 응답은 Started/SkippedBusy/NotStarted로 나눈다.

탐색의 `thread_unsubscribe` 및 화면 교체의 `reset_thread_event_state` 두 경계에서 cron 소유 일반 대화의 구독·이벤트 저장소·listener·승인 reply 소유권을 보존한다. 마지막 예약/실행 참조가 끝나면 비활성 대화의 구독을 해제한다. 기존 inactive 승인 UI와 origin-bound reply 경로를 재사용하며 신뢰·권한을 자동 승인하지 않는다.

응답 전 terminal과 중복·역순 응답은 attempt 및 정확한 owned turn으로 처리한다. remove는 미래 예약만 제거한다. cancel은 정확한 owned ID만 사용하고 active turn ID로 대체하지 않는다. interrupt ACK 뒤에도 terminal까지 소유권을 유지한다. 연결/이벤트 유실은 Unknown으로 중지하며 새 연결에서 자동 재전송하지 않는다. 정확한 구 method 거부만 BlockedUnsupported이며 같은 backend의 예약을 일괄 차단한다.

## 직접 실행한 검증

| 검사 | 결과 | 근거 |
| --- | --- | --- |
| 실제 production scheduler 모듈 단독 rustc 시험 | 6 PASS, build/test exit 0 | scheduler-test.log, scheduler-receipt.json |
| 격리 사본에서 stale-attempt guard 제거 mutation | 표적 시험 실패 exit 101을 탐지 | mutation-test.log, scheduler-receipt.json |
| 설치된 공식 CLI 0.165.0의 격리 stdio 새 method 호출 | 정확한 -32600 unknown variant, ordinary start 전송 0, turnStarted 관측 0, 정상 exit 0 | old-backend-result.json, old-backend-stdio.jsonl |
| CS 제약 및 test plan 구조·참조 검사 | READY(제품 검증 아님) | cs-constraints.json, engineering-test-plan.json |

공식 호환성 검사는 `CODEX_HOME`을 `checks/cron/old-backend-home`으로 격리했고 인증을 복사하지 않았다. 정확한 `Invalid request: unknown variant `turn/startIfIdle`, expected one of ...` 형태 및 -32601만 미지원으로 분류한다. 다른 -32600 전체를 미지원으로 간주하지 않는다.

## 남은 필수 검증

아래 시험 코드는 작성했지만 모두 NOT_RUN이다. native release의 자원 gate가 열리기 전에는 전체 Cargo check/test를 시작하지 않았다.

- `codex-app-server` 실제 core/stdio: 경쟁 idle 요청은 Started 1/SkippedBusy 1, busy 입력의 model/history 기록 0, 수동 steer 계약 보존.
- `codex-tui`: plain A 등록 → plain B 실제 화면 교체 → A 예약 실행, B draft 및 화면 모델 격리, A terminal/result store.
- `codex-tui`: side-parent/subagent 조건이 없는 inactive 일반 대화의 승인 표시·origin WebSocket 응답·store 정리.
- protocol serde 및 schema 생성/fixture 대조. 생성은 기존 `schema_fixtures::write_schema_fixtures_from_env`로 수행해야 한다. Windows raw Option 이름 문제를 피하는 새 named params/response를 사용했다.
- 실제 owned cancel과 manual turn 경합, terminal 직전·직후 취소, 마지막 참조 unsubscribe, 구독 실패 오류, reconnect 후 old epoch 응답은 실제 통합 fixture로 추가 확인해야 한다. 단독 scheduler 시험만으로 이 경계를 PASS라고 하지 않는다.
- 최종 실행 파일 build, patched backend 활성화, 실제 Windows 터미널·절전·부하·장시간 사용, 독립감사·설치·배포는 미완이다.

기존 추적 Rust 파일은 checkout의 CRLF를 유지하며 새 파일은 UTF-8(no BOM)/LF다. 기존 prompt/include_str assets를 바꾸지 않았다. 실행 명령은 각 재현 스크립트와 receipt에 있다.
