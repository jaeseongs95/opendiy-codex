# Rust 소스 구조와 r22 기준

기존 프로젝트 README, LICENSE, NOTICE는 그대로 보존했다. 이 문서는 가져온 공개 Rust 소스를 찾고 검사하는 안내다.

| 위치 | 내용 |
| --- | --- |
| `codex-rs/cli` | `codex-cli` crate, `codex` 실행 파일의 진입점 |
| `codex-rs/app-server` | `codex-app-server` crate 및 실행 파일 |
| `codex-rs/app-server-protocol` | App Server 요청·응답과 생성된 공개 스키마 |
| `codex-rs/core`, `codex-rs/codex-mcp`, `codex-rs/rmcp-client` | CLI와 서버가 공유하는 구현 |
| `codex-rs/Cargo.toml`, `codex-rs/Cargo.lock` | 전체 workspace와 고정 의존성 그래프 |
| `codex-rs/vendor`, `third_party`, `patches` | upstream에서 추적하는 소스·고지·빌드 패치 |
| `SOURCE_BASELINE.json`, `upstream/source-inventory.json` | 원본 revision, Git blob, 소스·게시 바이트 해시 |
| `MODIFICATIONS.md`, `upstream/r22.patch` | r22 변경 안내와 원래 Git diff |

공식 원본은 `openai/codex`의 `6905ecede7816e91373e9eabda3f6b0368fc747b`, r22 패치 기준은 `f74c7574ae39b95550d6b445f828ba1010787f74`다. 로컬 작업 폴더의 미검증 r23 변경은 포함하지 않는다. 별도 데스크톱 AppX·ASAR·추출 JS·실행 파일·사용자 프로필은 포함하지 않는다. 공개 upstream의 Rust 빌드에 포함되는 assets와 소스 테스트 fixture는 유지한다. 새 GitHub Actions workflow는 추가하지 않았다.

CLI와 App Server 두 디렉터리만 가져오면 workspace와 공유 path 의존성이 빠진다. 여기에는 전체 Cargo workspace를 보존했다. `mio 1.2.4`는 고정 lockfile의 공개 registry 패키지이며, 머신 전용 경로 패치를 추가하지 않았다. 외부 Git/registry 의존성을 내려받고 대상 OS의 빌드 도구를 준비하는 작업은 별도다.

Rust toolchain은 `codex-rs/rust-toolchain.toml`을 따른다. 소스 검사와 빌드 명령은 다음과 같다.

```sh
python -B scripts/source-import/verify.py
python -B scripts/source-import/test_verify.py
cd codex-rs
cargo metadata --no-deps --locked --offline --format-version 1
cargo build --locked -p codex-cli --bin codex
cargo build --locked -p codex-app-server --bin codex-app-server
```

`--offline` metadata는 필요한 의존성이 로컬에 준비된 환경에서만 성공할 수 있다. 검증 명령은 결과를 stdout으로 내보낸다. 실행 검증 receipt와 로컬 감사 기록은 이 공개 소스 저장소에 포함하지 않는다. 전체 release build, 실행 파일 시작, 네이티브 동작·성능·회귀 시험을 완료했다는 뜻은 아니다.

다시 추출하려면 필요한 두 커밋이 있는 별도 upstream Git 저장소를 준비하고 `python -B scripts/source-import/export.py --source-repository <upstream-repository>`를 실행한다. 현재 저장소의 Git 이력이나 작업 파일을 복사하지 않으며, 기존 파일 내용이 달라져 있으면 덮어쓰지 않는다. `upstream/r22-build-input-hashes.json`은 당시 빌드 입력의 해시만 담고 호스트 경로는 담지 않는다.

원래 Git mode가 symlink인 `codex-rs/vendor/bubblewrap/LICENSE`는 같은 디렉터리의 `COPYING`을 가리킨다. 소스 exporter는 Windows에서도 검사할 수 있도록 링크 대상 문자열을 기록하고, 커밋에는 원래 Git symlink mode를 보존한다. upstream의 실행 가능 파일 mode도 보존한다.

Windows에서 다시 export한 파일을 커밋할 때에는 `git add --all` 후 `python -B scripts/source-import/export.py --stage-modes`로 inventory의 Git mode를 index에 복원한다. 이 명령은 검토한 파일 바이트가 달라져 있거나 stage되지 않았으면 실패한다. 검증기는 Python 최적화 모드를 거부하고, Linux의 실제 LICENSE symlink와 Windows의 대체 파일을 구분해 검사한다.

`upstream/LICENSE`와 `upstream/NOTICE`는 upstream 원문이다. vendored bubblewrap의 Library GPL v2 고지, Ratatui의 MIT 고지 등 구성요소별 기존 라이선스는 해당 파일에 남아 있다. 모든 third-party 파일을 Apache-2.0으로 다시 지정하지 않는다.
