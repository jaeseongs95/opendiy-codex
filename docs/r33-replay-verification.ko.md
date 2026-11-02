# R33 메시지 재생 수정 검증 요약

고정 원본 소스는 `278f41f74f26b72cd2f3bf068400d8454a00ff75`, 공개 기준은 `3f2185d69aa14ff8cb9f0991f119bdc6f222de7e`다. 공식 0.166.0 기반 R32에서 완료 이력과 버퍼가 겹칠 때 같은 메시지를 두 번 표시하는 문제에 한정한 소스 후속 변경이다. Cargo 버전은 `0.166.0+native-r32.20261101`을 유지한다.

## 변경과 실패 경계

이전 후보 `eb47de18e688239e490f42e63658ade505566e00`의 합성 TUI 시험에서 동일 ID의 사용자 메시지·답변·완료 구분선은 `(2, 2, 1)`로 표시돼 기대값 `(1, 1, 1)`에 실패했다. 다른 ID의 동일 본문은 별도 메시지로 보존되는 시험이 통과했다.

수정은 `codex-rs/tui/src/app/replay_filter.rs`, `thread_routing.rs`, `tests/buffered_replay.rs` 세 파일이다. 완료 이력의 동일 thread·turn·item ID와 전체 payload를 확인한 뒤 재생 사본에서만 중복 본문을 억제한다. 에이전트 완료 이벤트의 질문과 완료 상태, 권한 요청, 새로운 item과 뒤따르는 완료 이벤트가 없는 델타는 보존한다. 같은 ID라도 payload가 다르면 억제하지 않는다. `TurnCompleted`의 최종 답변 fallback도 동일한 본문 억제를 적용한다.

## 실제 시험 결과

2026-11-02 고정 원본 소스의 TUI 시험 실행 파일에 아래 그룹 필터를 전달해 각각 실행했다. 각 실행의 종료 코드는 0이며 메타데이터의 소스 커밋·실행 파일 SHA-256과 실제 로그의 시험 이름·통과 수를 대조했다. 시험 실행 파일 SHA-256은 `fbc4b6c5c620d2cf80186d50e37754d407ceaaea1043cb3680ba5799b5600255`다. 원시 로그와 실행 파일 자체는 공개 소스에 포함하지 않는다.

| 시험 필터 | 통과 | 실패 | 무시 | 종료 코드 |
| --- | ---: | ---: | ---: | ---: |
| `app::tests::buffered_replay::` | 17 | 0 | 0 | 0 |
| `app::tests::pagination_completion_tests::` | 4 | 0 | 0 | 0 |
| `app::tests::history_hydration_tests::` | 8 | 0 | 0 | 0 |
| `chatwidget::tests::question_notifications_tests::` | 4 | 0 | 0 | 0 |
| 합계 | 33 | 0 | 0 | — |

동일 ID 단일 표시, 다른 ID의 동일 본문 보존, 여러 turn의 중복·완료 상태, 과거 델타 억제와 신규 item 표시, 질문·요청·미완료 델타 보존, 진행 중 turn과 변경된 payload 보존을 포함한다. 모든 TUI 시험의 전량 통과나 실제 앱의 부하 검증을 뜻하지 않는다.

재실행 시에는 저장소의 toolchain과 허용된 시험 환경을 준비한 뒤 다음과 같이 같은 필터를 사용할 수 있다. 이 명령으로 공개 checkout에서 Rust 시험을 새로 빌드·실행한 기록은 없다.

```sh
cd codex-rs
cargo test --profile dev-small --locked --target x86_64-pc-windows-msvc -p codex-tui --lib app::tests::buffered_replay:: -- --nocapture --test-threads=1
cargo test --profile dev-small --locked --target x86_64-pc-windows-msvc -p codex-tui --lib app::tests::pagination_completion_tests:: -- --nocapture --test-threads=1
cargo test --profile dev-small --locked --target x86_64-pc-windows-msvc -p codex-tui --lib app::tests::history_hydration_tests:: -- --nocapture --test-threads=1
cargo test --profile dev-small --locked --target x86_64-pc-windows-msvc -p codex-tui --lib chatwidget::tests::question_notifications_tests:: -- --nocapture --test-threads=1
```

## 공개 소스 출처 검증

기존 전체 Rust 작업공간을 유지한 채 고정 Git blob에서 재export했다. 이전 원본 소스 `c23bf2aaed0c31d147ce915d5478c926282a5482`와 현재 후보 사이의 export 대상 변경은 위 세 파일뿐이다. 공개 파일 본문은 수정 고지를 제외하면 현재 고정 원본의 세 파일과 바이트 단위로 일치한다. 다른 source inventory 파일의 공개 바이트와 기존 LICENSE·NOTICE·README 두 파일은 보존했다.

```sh
python -B scripts/source-import/verify.py
python -B scripts/source-import/test_verify.py
```

실제 공개 작업사본에서 두 명령을 실행했고 모두 종료 코드 0이었다. 출처 검증은 소스 9,401개·공식 대비 수정 526개·Cargo manifest 171개·작업공간 구성원 165개·경로 입력 294개·Rust literal include 132개·내장 프롬프트 38개를 확인했다. Windows 프롬프트 원시 해시 검사도 통과했다. 도구 회귀시험은 18개 중 17개 통과·이 환경의 실제 심볼릭 링크 생성 1개 건너뜀이며, R33 고지를 세 경로에만 적용하고 기존 R32 고지를 보존하는 검사도 포함한다.

출처 검증은 `SOURCE_BASELINE.json`과 inventory·공식 대비 patch의 해시 결속, 각 파일의 공개 바이트·원본 SHA-256·Git blob, 기존 고지와 새 고지 선택을 검사한다. exporter는 고정된 이전 공개 입력을 먼저 확인해 수정된 입력이나 별도 파일을 조용히 덮어쓰지 않는다. `--update`는 지정한 이전 공개 기준의 깨끗한 작업사본에서 사용한다.

공개 checkout의 Rust 빌드, 실제 CLI·Desktop에서의 전환·재개·부하 효과, 설치·릴리스와 자동 갱신 구현은 이번 검증에 포함하지 않는다. 원본 시험 통과와 공개 출처 검증을 새 실행 파일의 재현성·실제 앱 문제 해결 완료로 확대하지 않는다.
