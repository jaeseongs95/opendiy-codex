# Testing methods and validation history

자연어 예약 도구 연결은 [사용 안내](native-cli-cron.ko.md)를 참고하세요. 새 후보의 `model_cron`·`cron_arguments` 시험은 해당 호출·취소 경계를 검사하기 위한 표적이며, 이 공개 작업 트리에서 실행한 결과로 보고하지 않습니다. 현재 실행 파일에 설치되지 않았고 실제 모델의 자연어 도구 선택 평가도 실행하지 않았습니다.

Current R28 checks are in [R28 validation and limits](cli-r28-validation.md). The sections below preserve earlier checkpoints and do not describe a new R28 public-checkout build.

For the earlier R26 final-executable checks and actual public-checkout schema run, see the [2026-10-29 validation checkpoint](cli-validation-20261029.md). The export-handoff status below records the earlier point in time.

The earlier R26 snapshot described below was pinned to official `360319d8dee62d87e2d59fc0c21805d79519cfed` plus local `f665ccc64aaf6baee8fd137d8054f1daaae4a398`. The public export's offline file/provenance verifier and its synthetic safety checks are distinct from actual Rust component tests and live-app validation. No runtime/build receipt or operational log is published. At the earlier source-export handoff, a new build and Rust test run from the public checkout had **NOT_RUN**; the later public protocol schema run is recorded in the linked 2026-10-29 checkpoint.

## Separate local candidate results and exclusions

For the pinned local source candidate, 45 targeted test groups returned exit 0 and reported 353 passed test executions. This sum includes overlapping filters and is not a count of 353 unique tests. It is not a complete workspace test run and was not run from this new public export checkout.

The CLI main-binary unit tests cover the changed doctor code; TUI library tests cover the changed TUI code. The CLI library executable's 15 tests (login/debug sandbox) and the TUI integration executable's 13 tests (tmux resize/VT100) were not run in those groups; their relevant source had no patch diff. A protocol schema smoke test remains ignored. Permission results cover the policy-component tests only, not full native enforcement or expanded privileges.

A release build, release-executable behavior and installation/live-app checks are separate from these test groups. At this source-export handoff, release execution and installation are **NOT_RUN**. Public-checkout compilation and runtime testing are also **NOT_RUN**. Source export success and synthetic fixtures do not establish production cleanup, desktop responsiveness or memory/resource-growth outcomes. Original operational logs, local receipts and implementation/audit artifacts remain private.

## Reproducible checks

Run the source/provenance and exporter safety checks without Python optimization:

```sh
python -B scripts/source-import/verify.py
python -B scripts/source-import/test_verify.py
```

The source verifier distinguishes canonical Git bytes from Windows working inputs. For 38 embedded prompt files, it checks Windows raw SHA-256 against `upstream/windows-prompt-inputs.json` and canonicalizes only their line endings for the Git-blob comparison. The root attributes preserve the old repository rules, merge the selected upstream LF/generated rules and add native `text` rules only for those 38 prompts. An actual Windows Git materialization with `core.autocrlf=true` and `core.eol=native` matched all 38 pinned raw prompt hashes while preserving both compressed schema resources byte-for-byte. A Linux materialization and a new Rust build of the public checkout are **NOT_RUN**; no Linux CRLF rule is added.

The public-export follow-up for generated TypeScript preserves the original Rust schema comparison: it removes only the publication notice from generated fixture files and records that notice externally in MODIFICATIONS. The Python regression reads GENERATED_TS_HEADER from the actual Rust exporter, checks its position in the current nine modified fixtures and rejects a local comment before it. This is a source/contract guard, not a substitute for the actual Rust schema group below. That public-checkout group is **NOT_RUN** at this edit while the separate release build is active. Run it after that build finishes, with a separate target directory:

```sh
cargo test --manifest-path codex-rs/Cargo.toml --locked --offline --target x86_64-pc-windows-msvc --target-dir <separate-build-target> --profile dev-small -j 1 -p codex-app-server-protocol --lib schema_fixtures_tests -- --test-threads=1 --nocapture
```

The minimum group contains the TypeScript/JSON generated comparisons, stable/experimental precomputed export comparisons and the existing ignored write-fixtures test. Check that all four read/comparison tests pass and that only the write-fixtures test is ignored. No Rust generator or comparison is changed to accept arbitrary publication comments.

For actual Rust boundaries, use the pinned toolchain and lockfile from `codex-rs`. On Windows the following examples exercise native pipes/Jobs, cleanup fault injection, queued RPC cancellation, WebSocket ownership cancellation and a real background PTY process. Fixtures use temporary processes/directories and loopback servers, without a user profile or external service. The exact-name filters below are existing test names; an appropriate Windows MSVC build environment and dependencies must be prepared separately.

```sh
cd codex-rs
cargo test --locked -p codex-utils-pty --lib mcp_read_lifetime_owned_job_and_process_fences -- --test-threads=1
cargo test --locked -p codex-rmcp-client --lib output_pipe_preserves_bytes_and_closes_with_its_writer -- --test-threads=1
cargo test --locked -p codex-rmcp-client --lib actual_launch_joins_pending_cleanup_before_admitting_next_generation -- --test-threads=1
cargo test --locked -p codex-app-server --lib queued_mcp_read_accepts_cancel_before_handler_dispatch -- --test-threads=1
cargo test --locked -p codex-core --lib capacity_owner_change_cancels_websocket_trace_after_untraced_warmup -- --test-threads=1
cargo test --locked -p codex-core --lib background_terminal_real_startup_keeps_pid_pty_environment_and_one_spawn -- --test-threads=1
cargo test --locked -p codex-core --lib responses_retry -- --test-threads=1
```

A filter can match zero tests; check the reported count and individual test names, not only exit code. Additional regression coverage lives alongside these source files and in the app-server/core/TUI integration suites. Run the relevant platform suites before relying on the patch. These commands describe how to test; they do not assert that every test or public-checkout build passed. Full Windows CLI/App Server checks, installation/start-path validation, real-app workload observation and desktop performance/resource measurements remain separate acceptance boundaries.

The packaging helper's [README](../tools/windows-cli/README.md) describes an argv-only disposable executable test. Its generated executable/package/receipt must not be committed. Upstream fixture AWS identifiers/secrets are intentionally synthetic public data; no real credentials are needed for these fixtures.

## Historical r22 test record

The remainder preserves the earlier r22 test record. Its commits, counts and observations do not describe the current 0.165.0 snapshot or a test run from this new public checkout.

# Testing / 테스트 방법

## 범위

공식 기준은 `6905ecede7816e91373e9eabda3f6b0368fc747b`, 공개하는 r22 Rust 소스 기준은 `f74c7574ae39b95550d6b445f828ba1010787f74`다. 원래 빌드 입력과 게시용 수정 고지의 바이트 차이는 [SOURCE_BASELINE.json](../SOURCE_BASELINE.json)과 [MODIFICATIONS.md](../MODIFICATIONS.md)에 구분했다.

아래의 **기존 r22 native 시험**, **이번 소스 import 검사**, **별도 desktop 관측**은 서로 다른 범위다. 현재 공개 clone에서 전체 release build와 앱 동작을 새로 검증했다는 뜻은 아니다. 미검증 r23 변경은 게시 소스와 이 결과에 포함하지 않는다.

## 기존 r22 Windows native 시험

고정된 r22 후보에서 Rust 1.99.1과 Windows MSVC target으로 다음 검사를 실행했고 종료 코드 0을 확인했다.

```sh
cargo check --locked --offline --target x86_64-pc-windows-msvc -j2 -p codex-cli -p codex-app-server --all-targets
```

실제 Cargo 표적 시험은 총 18개가 통과했다.

| 시험 경계 | 통과 수 | 확인한 내용 |
| --- | ---: | --- |
| PTY Windows Job | 2 | 소유 자식 프로세스·Job 수명과 containment 실패 처리 |
| Windows IOCP stdio | 3 | 조용한 pipe, 읽기 취소, byte/EOF 보존 |
| Launcher process/Job | 3 | caller drop 뒤 정리, 실패 launch의 재시도 fence |
| App Server config/read | 4 | 단일 snapshot, cwd/project 문맥, 바뀐 account/policy 거부 |
| MCP cancel/connection ownership | 6 | 실제 owned future 취소, 완료 경합, queue·connection·ownership 경계 |

별도의 정상 Cargo 소켓 fixture는 현재 RMCP crate를 path dependency로 사용했다. IPv4/IPv6 IO, 자식에게 소켓이 상속되지 않음, 부모 drop 뒤 **시험 소켓**의 CLOSE_WAIT 0, 접속 거절, named-pipe IO·disconnect를 검사했다. App Server 시험과 같은 Mio artifact의 SHA까지 대조했다. 별도 fixture의 Tokio/RMCP 전체 dependency graph가 동일하다는 주장은 아니다. 운영 앱 전체의 CLOSE_WAIT 원인이나 UI 지연이 해결됐다는 증거로 사용하지 않는다.

## 이번 공개 소스 import 검사

```sh
python -B scripts/source-import/verify.py
python -B scripts/source-import/test_verify.py
cargo metadata --manifest-path codex-rs/Cargo.toml --no-deps --offline --locked --format-version 1
```

- 공개 소스 9,026개에 대해 고정 Git blob·게시 바이트·수정 고지 관계를 검사했다. Cargo path 입력 287개와 literal Rust include 131개에 누락이 없었다.
- Cargo metadata는 종료 코드 0이며 CLI와 App Server를 포함한 workspace 패키지 166개를 인식했다. 의존성이 준비된 로컬 환경에서 수행한 검사다.
- 검증기 자체 시험은 6개 중 5개 통과, 실제 OS symlink 생성 1개는 Windows 호스트 권한 제약으로 건너뛰었다. Windows 대체 파일, synthetic readlink, 잘못된 target·경로 이탈, Python 최적화 모드 거부는 검사했다. Linux 실제 checkout 시험을 완료했다고 표현하지 않는다.

## 별도 desktop 관측과 한계

과거 desktop 진단에서는 직접 입력과 대화 이동을 수행하며 Performance Trace를 수집하고, 1분 주기로 외부에서 메모리·TCP·프로세스 상태를 수집하고 실행 세대별로 대조했다. 이 관측은 Rust 표적 시험과 별개이며, 지연·OOM·운영 연결 누적이 완전히 해결됐다는 판정은 아니다.

이 저장소는 공개 Rust 소스만 제공한다. 비공개 desktop UI 소스·추출 자산은 제공하지 않으므로 이 소스만으로 전체 desktop 시험을 재현할 수는 없다. 원시 로그, 수집 DB, 프로필, 실행 영수증과 로컬 감사 기록은 게시하지 않는다. 여기에는 시험 방법과 확인 범위만 요약한다.

## English summary

Historical r22 Windows testing passed a locked/offline CLI and App Server all-targets check and 18 targeted Cargo tests. A separate socket fixture checked IO, inheritance, cleanup and named pipes; its Mio artifact matched the App Server test artifact, not the entire Tokio/RMCP graph. Source-import checks separately verified provenance, local build inputs and Cargo metadata. Verifier self-tests had five passes and one real-symlink skip. Desktop typing/navigation traces and external memory/TCP/process observations at one-minute intervals were separate diagnostics. A fresh public-clone release build, full desktop reproduction and complete resolution of performance or connection issues are not claimed. Private UI assets and raw operational evidence are excluded.

## 빈 미저장 대화 이력 복원 후속 후보

새 고정 소스 `67404f486047863ff7e56d7f47f4c350a872b483`는 `004c9113ff02c292998f1fa03355afe3dfe6d404` 이후 시험 3파일만 보강한 후보다. 제품 코드는 동일하다. 직전 `4f`의 화면·계약 시험은 통과했지만 전체 TUI 시험에서 1개가 실패했으며 시험 안내에 기록했다. 직전 `92d`의 06 TUI 컴파일은 시험 준비 호출의 `anyhow::Error`를 `eyre`로 자동 변환하지 못한 `E0277`로 종료 코드 101이었고 실행시험·일반 실행 파일 빌드는 미실행이다. `704`는 그 호출 한 곳을 인접 시험과 같은 명시적 준비 실패 `expect`로 바꿨다. `704`의 07 시험 컴파일은 통과했지만 대화 선택 시험은 1개 통과·2개 실패였고 일반 개발 빌드는 미실행이다. 이번 `674`는 설정 변경 알림과 `includeTurns=false` 생략 계약을 시험에 반영한 수정이다. 08 일반 개발 CLI·TUI 시험 컴파일과 영향 시험 66개, 고정 개발 실행 파일의 격리 시험 두 실행기는 종료 코드 0이었다. 전체 TUI를 새로 전량 통과한 결과는 아니다. 독립감사 08은 고정 소스·개발 실행 파일·66개 영향 시험·격리 실행에 한정해 PASS였다. 이후 최적화 CLI·별도 패키지 검사와 실제 Desktop 공유 실행도 확인했지만, 이에 대한 독립감사 09는 진행 중이며 설치·모바일·장시간 부하는 미완료다. 범위는 시험 안내에 구분했다. 이전 `9766` 후보의 표적 107개·전체 TUI 5,849개 통과와 고정 개발 실행 파일·감사 07 결과는 이번 후보의 통과 근거가 아니다.

`004c`의 실제 실행 과정은 다음과 같다. 실패를 포함한 결과이며 새 `4f`의 실행 결과와 구분한다.

| `004c` 단계 | 실제 결과 |
| --- | --- |
| 표적 컴파일 04 | 성공, 종료 코드 0 |
| 서버 재개 시험 | 76개 통과·0개 실패, 종료 코드 0 |
| 이력 계약 시험 | 5개 통과·0개 실패, 종료 코드 0 |
| 이력 화면 시험 | 7개 통과·1개 실패, 종료 코드 101 |
| 전체 TUI 04·일반 개발 CLI 04 | 미실행 |

실패한 시험은 실시간 턴이 조회 스냅샷의 `turns`에도 들어간다고 가정했다. 기존 `thread_events.rs`는 `set_turns`로 채운 조회 스냅샷과 실시간 알림을 보관하는 `snapshot.events`를 분리한다. 새 시험은 `Paste`·Enter → 앱 대기열의 `CodexOp` → 서버 → 정확한 대화·턴 알림 보존 → 기존 활성 대화 이벤트 처리 경로 → 화면 표시를 통과하도록 보강했다. 첫 입력과 `Done`이 각각 한 번 표시되는지, 권한·세션·연결이 유지되는지, 턴 시작 원격 호출과 모델 요청이 중복되지 않는지 검사하도록 작성했다. 이 시험 변경은 직전 `4f`의 05 실행에서 화면 시험 8개 통과로 확인했으며, 현재 후보의 실행 통과를 뜻하지 않는다.

`4f`의 05 컴파일은 종료 코드 0이었다. 화면 시험 8개와 이력 계약 시험 5개는 각각 실패 0개·종료 코드 0이었다. 전체 TUI는 556.57초 동안 5,851개 통과·1개 실패·4개 무시, 종료 코드 101이었다. 실패한 `attach_live_thread_for_selection_rejects_empty_non_ephemeral_fallback_threads`는 실제 실시간 재개가 성공한 대화를 빈 재생 대체 경로의 실패로 기대했다.

`92d`에서 보강한 시험은 그 성공 경로에서 실시간 연결·권한·원래 세션과 백엔드·실시간 설정 알림 유지를 검사하고, 별도 시험에서 재개 응답을 명시적 `-32603` 오류로 만들어 비어 있는 저장 이력의 재생 대체 경로가 거부되는지 검사한다. `4f`에서 추가된 실제 입력·이벤트 처리 시험은 유지한다. 06 TUI 컴파일은 시험 준비 호출의 `E0277` 오류로 종료 코드 101이었고 실행시험·일반 개발 빌드는 미실행이다. `704`는 해당 호출의 `?`를 인접 시험과 같은 준비 실패 `expect`로 바꾼 시험 전용 수정이다. 07 TUI 시험 컴파일은 종료 코드 0이었다. 화면 시험 8개·이력 계약 시험 5개·공유 세션 시험 구성 50개는 각각 실패 0개·종료 코드 0이었다. 대화 선택 시험은 1개 통과·2개 실패, 종료 코드 101이었고 일반 개발 빌드는 미실행이다. 한 실패는 같은 설정값의 중복 알림이 억제되는 원본 계약을 빠뜨린 것이고, 다른 실패는 `includeTurns=false`가 직렬화에서 생략되는 계약을 빠뜨린 것이었다. 이번 `674`는 실제로 다른 추론 강도를 설정해 알림을 확인하고, 요청을 `ThreadReadParams`로 역직렬화해 기본값을 확인하도록 시험 한 파일만 보강했다(16줄 추가·3줄 삭제). 제품 변경은 없다. 08 영향 재시험에서는 아래 결과를 확인했다. 이전 실패 기록은 유지하며 영향 시험 통과를 새 전체 TUI 전량 통과로 해석하지 않는다.

`674`의 08 일반 CLI 개발 빌드는 `dev-small`·비최적화·잠금 파일 유지 조건에서 3분 18초, 종료 코드 0이었다. TUI 시험 컴파일도 종료 코드 0이었다. 화면 8개·이력 계약 5개·선택 연결 3개·공유 세션 시험 구성 50개, 합계 66개가 각각 실패 0개·실행기 종료 코드 0이었다. 전체 TUI 원자료는 여전히 05의 5,851개 통과·1개 실패·4개 무시이며, 그 뒤 시험 코드만 바뀐 범위의 영향 재시험 결과를 구분해 기록한다.

같은 소스에 결속된 고정 개발 실행 파일의 SHA-256은 `e8087aa0d066f69d1ac9e8ff1a83c8435b06c1360c94a8d3cdeb646490d7e912`다. 설치를 바꾸지 않고 별도 시험용 환경에서 두 이력 형식(`legacy`·`paginated`)의 초기 이력 조회를 실행했다. 기존 `-32600` 코드·문구와 새 `data` 표식, 동일 세션·백엔드 유지, 프록시 종료 뒤 서버 생존을 확인했고 마지막에는 시험 소유 서버만 회수했다. 모델 POST는 0건이고 인증 파일은 생성되지 않았다. 이력 조회 시험 실행기의 종료 코드는 0이었다.

같은 실행 파일에 `--logging`을 사용한 용량 오류 시험 실행기도 종료 코드 0이었다. 정상 재시도는 모델 요청 2건·종료 코드 0·기록 581개, 부분 응답 실패는 재전송 없이 요청 1건·예상 종료 코드 1·기록 561개, 허용 대기 상한 초과는 재시도 없이 요청 1건·예상 종료 코드 1·기록 552개였다. 기록 개수 자체를 제품 완료 근거로 삼지 않는다.

독립감사 08은 소스 `674`·위 고정 개발 실행 파일·영향 시험 66개·격리 실행 범위에서 `PASS(한정)`였다. 공개 사본 자체의 빌드·설치나 제품 전체 완료 판정은 아니며, 아래 실제 Desktop 공유 실행은 이 감사와 별도로 확인한 결과다.

최신 AgentGrid `3116dde`와 CLI `674`·고정 개발 실행 파일을 사용한 실제 공유 실행 1회에서 8개 검사가 모두 통과했고 실행기는 종료 코드 0이었다. 두 빈 대화의 active/idle 상태에서 실제 타이핑 표시·삭제, 같은 백엔드와 Unix 소켓의 실제 TUI, Shift+Enter 두 줄·단독 Enter 0회·모델 요청 0건을 확인했다. 한 대화를 닫아도 다른 대화가 유지됐고 앱 종료 뒤에도 백엔드 세대·등록·로드된 대화가 보존됐다. 시험 소유 서버만 명시적으로 회수했으며 관측한 잔여 앱 자식은 0개였다. 이력 미완료 경고는 남아 있어 이력 완료나 실제 메시지의 모델 응답, 휴대전화 실연결·물리 IME 통과를 뜻하지 않는다.

최적화 `release` 01은 `client_name` 누락 `E0063`으로 종료 코드 101이었다. 외부 작업 트리 경로가 남은 이전 `rmeta`를 확인한 뒤 관련 5개 패키지 캐시만 같은 드라이브에 보존 이동했고, 제품·빌드 명령을 바꾸지 않은 `release` 02는 소스 `674`·`opt-level=3`에서 종료 코드 0이었다. 캐시 재사용의 미확인 내부 메커니즘은 단정하지 않는다. 고정 최적화 실행 파일 SHA-256은 `2fae2340293336ed0827cb139349998605bb9767c4aad80b3168a9440f9547c2`다.

최적화 실행 파일의 이력 조회·용량 오류 시험 실행기는 각각 종료 코드 0이었다. 두 이력 형식에서 기존 오류 코드·문구와 구조화 표식, 같은 백엔드와 프록시 종료 후 서버 유지, HTTP·모델 요청 0건·인증 파일 미생성을 확인했다. `--logging` 용량 시험은 정상 재시도 요청 2건·종료 0, 부분 응답과 대기 상한 초과 각각 요청 1건·예상 종료 1이었다. 기록은 각각 579·561·552개, 배치는 5·1·1개였다. 별도 패키지는 47파일 해시가 일치했고 두 `.cmd` 실행기의 버전 명령이 종료 0이었다(시험용 홈 부재에 따른 별칭 생성 경고 있음). 패키지 영수증 SHA-256은 `7b8663a018b6ef252790cf63ad620f020304a4b2202f4feb198fb41c01f272ae`이며 설치 전환은 하지 않았다.

같은 AgentGrid `3116dde`와 위 최적화 CLI의 별도 실제 공유 실행 1회도 8개 검사 통과·실행기 종료 0이었다. 두 빈 대화의 입력 표시·줄바꿈·공유 백엔드 수명 보존을 확인했으며 HTTP·모델 요청은 0건이었다. 이력 미완료 경고는 유지됐고 모바일은 `disabled`·`pairing=null` 상태 조회만 확인했다. 새 독립감사 09는 진행 중이며 최종 PASS는 아직 없다. 전체 TUI 전량 재실행, 실제 모델 응답·휴대전화 연결·물리 IME·강제 경합·장시간 부하, 설치·배포는 미완료다. 원시 운영 로그·실행 파일·운영 경로·대화 식별자는 게시하지 않는다.

서버는 같은 로컬 작성자가 실제로 로드돼 있고 영속 파일 생성을 미룬 상태인 초기 이력 조회의 `-32600` 오류에만 `kind=historyProjectionUnavailable`, `reason=unmaterialized` 표식을 더한다. 기존 코드·메시지는 유지하며 삭제된 저장 이력, 커서·고정점 요청과 다른 오류에 이 예외를 넓히지 않도록 작성했다.

TUI는 성공한 재개와 원래 세션·연결을 유지하면서 이력 미조회 상태를 보존하고 입력·실시간 이벤트를 계속 처리하도록 변경했다. 오류를 한 번 표시하거나 새 이벤트를 받았다는 이유로 이력을 조회 완료로 바꾸지 않는다. 빈 재생에도 종료 이벤트를 보내 현재 표시 대화의 이력 상태를 반영하고, 이전 커서가 없는 `Home` 동작이 로딩 상태에 고정되지 않도록 했다. 명시적인 이력 조회 성공 뒤에만 해당 미조회 상태를 해제하도록 작성했다.

작성한 회귀는 두 이력 형식의 실제 재개·첫 입력·원래 세션 식별자·실시간 이벤트와 이력 미조회 상태, 다른 대화로 전환한 뒤 지연된 빈 재생 종료 이벤트, 커서 없는 `Home`, 명시적 재조회 성공을 검사한다. 정확한 오류 코드·구조화 표식 없이 문구만 같은 오류와 전송 오류는 예외로 인정하지 않는 검사도 포함한다. 아래는 새 후보의 시험 방법이며 통과 기록이 아니다.

```sh
cd codex-rs
cargo test --locked -p codex-tui --lib unmaterialized_history_recovery_requires_exact_structured_contract
cargo test --locked -p codex-tui --lib live_unmaterialized_resume_keeps_session_input_and_incomplete_history
cargo test --locked -p codex-tui --lib generic_projection_unavailable_survives_resume_and_error_display
cargo test --locked -p codex-app-server --test all suite::v2::thread_resume::thread_resume_rejoins_unmaterialized_thread_from_another_connection
cargo test --locked -p codex-app-server --test all suite::v2::thread_resume::thread_resume_metadata_preserves_deleted_rollout_contract
```

아래 결과는 이전 `9766` 후보에 한정한다. 새 후보의 완료 여부와 실제 화면·자연어 도구 선택·모바일·장기 부하·최적화 빌드·설치에 대한 판단은 별도로 남아 있다.

## 공유 로컬 서버 후속 회귀시험—검증 범위

이전 고정 원본 소스 `9766a8b7b8c4d56d7076367ddc12f2917a3df197`에서 다음 시험과 빌드를 완료했다. 공개 사본에는 수정 고지가 추가되므로, 이 결과가 새 공개 사본 자체의 빌드·실행을 뜻하지는 않는다. 원시 로그·실행 파일·운영 경로·인증 자료는 게시하지 않는다.

| 확인 범위 | 실제 결과 |
| --- | --- |
| 예약·재접속·재개·도구·프록시·소켓 표적 8개 그룹 | 107개 통과·0개 실패, 각 그룹 종료 코드 0 |
| 전체 TUI 시험 실행 파일 | 5,849개 통과·0개 실패·4개 무시, 종료 코드 0 |
| 일반 CLI 개발 빌드 | `dev-small`, `--offline --locked`, `-j2`, 종료 코드 0. 최적화한 `release` 빌드가 아님 |
| 고정 개발 실행 파일의 격리 시험 | 기본 프록시의 두 이력 형식 재개와 `--logging` 용량 오류 세 조건 통과, 시험 실행기 종료 코드 0 |

일반 개발 실행 파일의 SHA-256은 `7b2e4c2d38a38d7a1187f02673177ae370c1e9ceb252cf2370ccf09e27b68a1b`다. 전체 TUI 결과는 같은 소스의 시험 실행 파일에서 얻었으며, 이 일반 실행 파일에서 5,849개 시험을 다시 실행했다는 뜻이 아니다. 기존 설치 실행 파일은 바꾸지 않았다.

처음 일반 개발 빌드는 같은 소스에서 종료 코드 101과 `RemoteControlStartConfig`의 `client_name` 누락 오류로 실패했다. 현재 소스에는 그 필드가 없는데 이전 `app-server-transport` 메타데이터가 재사용된 사실과 일치한다. 관련 `app-server`·transport·TUI 지문 6개만 보존 이동한 뒤 같은 소스·잠금 파일·빌드 인자로 재컴파일해 종료 코드 0을 확인했다. 전체 캐시를 삭제한 것은 아니며, 잘못된 메타데이터가 처음 유입된 경위와 향후 다른 작업 트리의 캐시 재혼입 방지는 미확인이다.

격리 프록시 시험은 새 시험용 홈과 로컬 제공자를 사용했다. `legacy`·`paginated` 각각에서 첫 입력 전 대화의 `thread/read`와 `thread/resume(excludeTurns=true)`가 같은 대화·세션 식별자를 반환하고 두 커서가 `null`임을 확인했다. 각 형식의 프록시 두 개를 회수한 뒤에도 같은 서버가 유지됐다. 마지막에는 시험이 직접 만든 서버만 종료했고 모델 요청은 0건이었다. 첫 격리 시도는 시험용 소켓 경로 길이 제한으로 실패했으며, 제품 코드를 바꾸지 않고 짧은 새 시험 경로로 다시 실행했다. 이 결과는 실제 TUI의 전체 이력 복원 검증과 구분한다.

용량 오류 시험은 새 시험용 홈, 인증이 필요 없는 로컬 제공자, `--no-daemon exec --sandbox read-only`와 `--logging`을 사용했다. 정상 재시도는 모델 요청 2건·종료 코드 0, 부분 응답 후 실패는 재전송 없이 요청 1건·종료 코드 1, 허용 대기 범위를 넘는 `Retry-After: 300`은 기다리거나 재시도하지 않고 요청 1건·종료 코드 1이었다. 뒤의 두 종료 코드 1은 예상한 거부 결과이며 세 조건을 평가한 시험 실행기는 종료 코드 0이다.

독립감사 07은 소스 변경 없는 캐시 복구, 고정 개발 실행 파일의 소스·빌드 결속과 제한된 격리 실행, 기존 시험 근거 보존 범위에서 `PASS`였다. 전체 제품 완료·설치 가능 상태와 미래 작업의 캐시 혼입 차단 보장은 `BLOCKED`다. 실제 TUI의 빈 미저장 대화 이력 복원, 모델의 자연어 예약 도구 선택, 실제 모바일의 동일 서버 사용, 장시간 동시 부하, 최적화한 `release`, 설치·배포와 바탕화면·스케줄러 대상 정합성은 미검증이다.

이전 `921d404013da2035f90e9ab055acadc957a88592`의 전체 TUI 시험은 5,845개 통과·4개 실패·4개 무시, 종료 코드 101이었다. MCP 필수 메타데이터 누락 2건과 새 예약 도구의 목록·`deferLoading` 기대값 누락 2건을 보강했다. 이어진 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609`의 표적 실행도 100개 통과·7개 실패였고 8개 그룹 중 3개만 통과했다. `9766`의 결과는 그 후 미저장 대화의 커서 저장소 조건, 실제 대화의 기록 준비, 직렬화 기본값, 도구 목록 스냅샷과 임시 접근 제어 목록 시험을 수정한 고정 소스에 대한 것이다. 이전 후보 `eab9e295e54453bc30e00ae93c85d39ba738f596`의 감사 05 실패를 취소하거나 전체 소스 감사 통과로 바꾸는 판정은 아니다.

아래 필터는 공개 소스에서 대응 시험을 실행하는 방법이다. 표적 시험 수와 전체 TUI 시험 수는 서로 겹치므로 합산하지 않는다.

프록시 시험은 Windows에서 넓은 디렉터리를 접속 전에 거부하고, 없는 디렉터리를 생성·복구하지 않으며, 실제 운영체제 토큰에 따라 연결 상대를 검사한 뒤 바이트 왕복 또는 전송 전 거부를 확인하도록 작성했다. 상대 토큰을 조회하는 PowerShell 자식은 숨김으로 실행한다.

- `implicit_proxy_rejects_broad_directory_before_connecting`
- `implicit_proxy_does_not_create_or_repair_missing_directory`
- `implicit_proxy_authenticates_real_peer_before_transferring_bytes`

재개 시험은 두 이력 모드의 이력 요구 거부 4사례, 다른 실제 연결의 미저장 대화 재접속 2사례, SQLite 사용 여부에 따른 기존 삭제 파일 조회 계약 2사례, 미로딩 런타임·임의 UUID 거부 2사례다. SQLite 메타데이터가 `Some`인 로드된 대화의 메타데이터 전용 재개는 기존 조회 계약을 유지하며 파일 삭제만으로 거부한다고 주장하지 않는다. 마지막 두 사례는 각 이력 모드에서 다른 프로세스에만 로드된 미저장 대화와 임의 UUID를 모두 검사한다.

- `thread_resume_rejects_unmaterialized_thread_history`
- `thread_resume_rejoins_unmaterialized_thread_from_another_connection`
- `thread_resume_metadata_preserves_deleted_rollout_contract`
- `thread_resume_metadata_does_not_create_missing_runtime`

```sh
cd codex-rs
cargo test --locked -p codex-stdio-to-uds --lib implicit_proxy_
cargo test --locked -p codex-app-server --test all suite::v2::thread_resume::thread_resume_rejects_unmaterialized_thread_history
cargo test --locked -p codex-app-server --test all suite::v2::thread_resume::thread_resume_rejoins_unmaterialized_thread_from_another_connection
cargo test --locked -p codex-app-server --test all suite::v2::thread_resume::thread_resume_metadata_preserves_deleted_rollout_contract
cargo test --locked -p codex-app-server --test all suite::v2::thread_resume::thread_resume_metadata_does_not_create_missing_runtime
cargo test --locked -p codex-tui --lib model_cron_mcp_metadata_and_cancelled_queue_fail_closed
cargo test --locked -p codex-tui --lib reconnect_keeps_other_backend_mcp_cron_on_the_new_app_queue
cargo test --locked -p codex-tui --lib external_transport_registers_dynamic_tools_and_finds_task_mentions
cargo test --locked -p codex-tui --lib creation_attaches_to_original_task_and_survives_service_restart
cargo test --locked -p codex-tui --lib -- --test-threads=1
```
