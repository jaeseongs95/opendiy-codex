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

## 공유 로컬 서버 후속 회귀시험—미실행

고정 소스 `9766a8b7b8c4d56d7076367ddc12f2917a3df197`의 후속 후보는 컴파일 중이며 수정 후 제품 시험·독립감사 통과·설치는 미완료다. 프록시 3개 시험 함수의 3사례와 재개 4개 함수의 10사례, 총 13사례를 작성했으며 새 제품 시험 실행은 0건이다. 아래는 실행할 시험 필터이며 통과 기록이 아니다. 소스 내보내기 검증기와 검증기 자체 시험은 제품 시험과 별개다.

이전 후보 `eab9e295e54453bc30e00ae93c85d39ba738f596`의 독립감사 05 결과는 `FAIL`이었다. 현재 후보 `9766a8b7b8c4d56d7076367ddc12f2917a3df197`는 그 판정을 이어받지 않는다. 새 후보는 컴파일 중이며 수정 후 제품 시험은 아직 실행하지 않았다. 독립감사를 통과하지 않았고 설치도 완료하지 않았다. 이전 갱신은 SQLite 설정별 회귀·MCP 시험 요청·판정 및 예약 도구 노출 기대값을 보강했고, 이번 갱신에는 미저장 대화의 커서 저장소 조건 수정도 포함된다.

이전 자연어 예약 후보 `921d404013da2035f90e9ab055acadc957a88592`의 전체 TUI 시험은 5,845개 통과·4개 실패·4개 건너뜀으로 종료 코드 101이었다. 실패 중 2개는 MCP 요청의 필수 메타데이터 누락으로 `a3edca405d1eddf138f549764459394603576804`에서 보강했다. 나머지 2개는 새 `cron` 노출을 빠뜨린 도구 목록·`deferLoading` 기대값으로 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609`에서 보강했다. 새 후보는 컴파일 중이며 수정 후 제품 시험은 아직 실행하지 않았다. 독립감사를 통과하지 않았고 설치도 완료하지 않았다.

직전 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609` 후보의 표적 실행은 100개 통과·7개 실패였고, 8개 실행 그룹 중 3개 통과·5개 실패였다. 이번 `9766a8b7b8c4d56d7076367ddc12f2917a3df197` 후보는 그 실패에 대한 수정본이며 아직 수정 후 실행 결과가 없다. 앞선 결과를 새 후보의 통과 근거로 사용하지 않는다.

이번 제품 변경은 저장 메타데이터가 없는 미저장 대화의 페이징 재개에서 커서 저장소를 붙이지 않는 조건으로 한정한다. 나머지는 실제 원격 대화의 기록 준비와 같은 `sessionId` 확인, `deferLoading=false` 필드 생략·역직렬화 기본값 검사, 새 `cron` 도구가 포함된 작업트리 목록 스냅샷, 임시 접근 제어 목록 시험의 `Set-Acl`을 `icacls`로 바꾸는 보강이다. 접근 제어 목록 시험은 감사 권한을 요청하지 않는다. 빈 미저장 대화의 실제 화면 전환까지 해결됐다고 주장하지 않는다.

MCP 큐 회귀는 필수 `io.modelcontextprotocol/protocolVersion`·`io.modelcontextprotocol/clientCapabilities` 메타데이터, 정확한 부정 응답 코드·메시지, 큐 도착 전 응답 본문 판정을 보강했다. 제품 전달 성공이나 새 후보의 시험 통과를 주장하지 않는다.

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
```
