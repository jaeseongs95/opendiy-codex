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
