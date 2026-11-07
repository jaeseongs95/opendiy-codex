# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

공식 기능을 재사용하고 확인된 결함을 고치는 비공식 Windows Codex CLI·Rust App Server 배포판입니다.

[최신 릴리스 다운로드](https://github.com/jaeseongs95/opendiy-codex/releases/latest) · [소스 빌드](docs/source-import.md) · [검증](docs/TESTING.md) · [변경 내역](MODIFICATIONS.md)

## 버전 상태

| 버전 | 공식 기반 | 상태 |
| --- | --- | --- |
| R39 | 0.167.0 | 동결된 R38 기반의 압축 변경 전용 개발 후보입니다. 요약 기준을 바꾸고 압축 후 완료된 작업의 원문 재첨부를 막습니다. 시험을 진행 중이며 최종 실행 파일과 릴리스 발행은 미완료입니다. 실제 모델의 요약 품질은 미검증입니다. [범위와 한계](docs/native-cli-r39.ko.md)를 참고하십시오. |
| R38 | 0.167.0 | [정식 발행·동결(`v0.167.0-r38.20261106`)](https://github.com/jaeseongs95/opendiy-codex/releases/tag/v0.167.0-r38.20261106). 최종 패키지 검증은 태그별 릴리스 노트를 참고하십시오. 발행 여부와 PC에 설치된 버전은 별도로 확인합니다. |
| R37 | 0.166.0 + 선별 공식 변경 | 정식 발행·동결(`v0.166.0-r37.20261104`). 소스 `2289ae3aa847cd64e7e987f56180160ce67a8694`: daemon 버전 선택 보호, 범위를 명시한 메모리 저장·통합, 공유 언어 설정, 플러그인 캐시 처리, 재연결 중복 필터, 선택 가능한 하위 에이전트 표시와 입력 처리를 수정했습니다. 개발 시험에는 입력 회귀 81개와 영어·한국어 UI의 Desktop 제출·저장·재시작 복원 네 사례가 포함됩니다. Managed Windows 메모리 정책과 지원 동작이 없는 작업 중 연결 종료 사례는 이 검증 범위에 포함되지 않습니다. 최종 패키지 검증 결과는 태그별 릴리스 노트에서 확인하며 소스 사본·개발 시험을 설치 완료로 해석하지 않습니다. |
| R36 | 0.166.0 + 선별 공식 변경 | R35 변경을 통합한 Windows 정식 릴리스 발행 완료. 발행 이후 동결합니다. |
| R35 | 0.166.0 | 별도 릴리스 없이 R36에 통합합니다. |
| R34 | 0.166.0 + 선별 공식 변경 | Windows CLI·독립 App Server·전체 패키지 발행 완료. 발행 이후 동결합니다. |
| R33 | 0.166.0 | 과거 패키지. Windows updater 결함은 R34에서 수정했습니다. |
| R32 | 0.166.0 | 과거 패키지. |
| R31 | 0.166.0 | 별도 발행 없이 R32에 통합했습니다. |

공개 Windows 패키지와 체크섬은 [릴리스 목록](https://github.com/jaeseongs95/opendiy-codex/releases)에서 확인하십시오. 소스 병합·개발 빌드만으로 발행 여부나 PC에 설치된 버전을 판단할 수 없습니다. 태그별 릴리스 노트에서 최종 실행본 검증 결과를 확인하고 설치 후 실행 경로와 버전을 대조하십시오. R36에 통합된 메모리·언어·업데이트 동작의 배경은 [R35 안내](docs/native-cli-r35.ko.md)를 참고하십시오.

## 기능

- 취소 전파, capacity 오류의 제한된 재시도, 소유 하위 프로세스 정리
- 대화별 컨텍스트·service tier, 메모리 활성화·정정, 하위 에이전트 표시와 확인 후 정리
- CLI 실행 동안 유지되는 예약: 대화가 바쁘면 해당 회차를 건너뛰고 CLI 종료 시 예약이 사라집니다.
- R36: 기본 1행·펼침 최대 3행의 하위 에이전트 표시, Cloud 환경 조회 오류 구분, 읽기 전용 `codex ci` 관측과 fast 전파·압축 단계 계측
- 모델 문맥과 분리한 화면 이력 제한, 전역에서 선택하는 영어·한국어 UI
- 메모리 버퍼를 사용하는 비동기 진단과 완성된 개별 배치 파일 게시
- 이전 패키지를 보존하며 진행 중인 공유 daemon을 자동 교체하지 않는 launcher·updater

R36에 통합한 변경은 선택 capability 선언, 실제 thread 연결, 모델 turn 완료를 구분합니다. 캐시 수정은 완료 usage 계측 누락을 막는 것이며 서버 캐시 정책을 바꾸거나 실제 적중률 개선을 입증한 것은 아닙니다.

## Windows에서 실행

1. 릴리스의 전체 ZIP과 `SHA256SUMS.txt`를 받습니다.
2. SHA-256을 확인하고 패키지 `USAGE.ko.md`를 따라 설치합니다. 기존 설치는 보존합니다.
3. 기존 공유 서버와 분리한 엔진은 `codex-local.cmd`로 실행합니다.

전체 ZIP을 권장하며 폴더 구조를 유지해 압축을 풉니다. 개별 `codex.exe`·`codex-app-server.exe`만으로는 전체 패키지가 구성되지 않습니다. `functions`를 포함한 code-mode 도구를 사용하려면 같은 릴리스의 `codex-code-mode-host.exe`를 **실제로 실행하는 EXE와 같은 폴더**에 두어야 합니다. 파일 이름을 바꾸지 마십시오. 이 파일이 없으면 code-mode host 시작이 `os error 2`로 실패할 수 있습니다.

R38은 이 helper를 개별 자산으로 제공하지 않았으므로 R38 전체 ZIP에서 가져와야 합니다. R39은 `codex-code-mode-host.exe`를 개별 자산으로도 제공할 계획이며 아직 발행 전입니다. 음성 등 추가 리소스까지 사용하려면 전체 ZIP이 필요합니다. 기본 CLI는 기존 서버에 접속할 수 있으므로 선택한 CLI와 실제 서버의 식별 및 해당 서버 옆의 helper 배치를 따로 확인합니다.

## 대화에서 입력하는 명령

다음은 PowerShell 명령이 아니라 CLI 대화 입력 명령입니다.

| 명령 | 용도 |
| --- | --- |
| `/cron add --every 10m -- 빌드 완료 여부 확인` | 반복 프롬프트 예약 |
| `/cron list` · `/cron remove 1` | 목록·삭제. 실제 예약 ID를 지정합니다. |
| `/context 120000` · `/context reset` | 해당 대화의 컨텍스트 설정 |
| `/memory view` · `/memory enable on` | 메모리 조회·명시적 활성화 |
| `/subagents` | 하위 세션 조회 |
| `/background` 또는 `F6` | 지원하는 터미널 작업을 백그라운드로 전환 |
| `/reload_plugin` | 플러그인 다시 연결 요청 |
| `/language ko` · `/language en` | 다음 실행부터 사용할 전역 언어 저장 |

컨텍스트는 모델 한도를 올릴 수 없습니다. 기억 정정 저장과 요약 통합은 다른 단계입니다. 자연어 제어는 실행 중인 CLI가 제공하는 도구를 사용합니다. plugin reload 응답은 모든 MCP 서버 준비 완료의 증명이 아닙니다.

PowerShell에서는 `codex update --check`, `--status`, `--enable-updates`, `--disable-updates`, `codex update`를 사용합니다. 지원하는 설치 패키지가 필요하며 [updater 안내](tools/windows-cli/UPDATE.ko.md)를 참고하십시오.

## OpenDIY Desktop과의 조합

[OpenDIY Desktop](https://github.com/jaeseongs95/opendiy-desktop)은 데스크톱 UI·다중 세션을 담당합니다. 이 저장소는 CLI·App Server·패키지를 담당하며 두 프로젝트가 프로토콜과 버전 호환성을 함께 검증합니다.

**Desktop 1.1.0의 OpenDIY 확장기능 최소 지원 버전은 OpenDIY CLI R36과 App Server R36입니다.** 발행된 R36 버전은 `0.166.0+native-r36.20261103`이며, 같은 배포판의 전체 패키지 사용을 권장합니다. R28을 포함한 R36 이전 버전은 지원 대상에서 제외합니다. 공식 Codex CLI 0.166.0은 제한된 기본기능 호환만 확인했으며 OpenDIY 확장기능은 보장하지 않습니다.

기존 서버·저장 데이터·진행 중 작업은 자동 교체 없이 보존합니다. 이 보존 정책과 지원 버전 범위는 별개입니다. 아래 격리 시험은 검증 이력이며 R28 보존 시험의 통과가 R28 정식 지원을 뜻하지 않습니다.

| CLI·서버 | Desktop | 검증 범위 |
| --- | --- | --- |
| R33 | 1.0.2 | 과거의 제한된 공동시험. R33 릴리스 기록 참고. |
| R34 | 해당 릴리스에서 시험한 후보 | 시작·격리 엔진·catalog·복원 검사. R34 릴리스 기록 참고. |
| R36 시험용 CLI·서버 | 1.1.0 후보 `5eda4e5` | 격리 환경의 GUI 입력·한글 IME/붙여넣기·HTTP/SSE·동일 backend 완료·정상 종료·재시작 복원 검사를 통과했습니다. 이 결과는 이 행의 시험용 실행본에 한정됩니다. |
| 공식 Codex CLI 0.166.0 | 같은 1.1.0 후보 | 격리 기본기능 검사 6개 통과. OpenDIY 확장기능은 보장하지 않습니다. |
| R36 시험용 CLI·격리 R28 서버 | 같은 1.1.0 후보 | 격리 보존 검사 7개 통과. 과거 시험 이력이며 R28은 지원 대상에서 제외합니다. |

접속·initialize 성공이나 버전 문자열 일치만으로 모든 기능의 호환성을 보장하지 않습니다. 실제 모바일 연결·장시간 부하·캐시 효과는 별도 근거가 필요합니다. 파일 생성은 클라우드 업로드 완료가 아니며 화면 이력 제한은 전체 메모리 상한이 아닙니다.

## 빌드와 라이선스

[소스 빌드](docs/source-import.md) · [Windows 패키징 도구](tools/windows-cli/README.md). Git에는 소스·빌드 도구를, Releases에는 실행 자산을 게시합니다. 소스 출처 검증은 공개 checkout의 실제 빌드를 대신하지 않습니다. 인증정보·운영 로그는 게시하지 않습니다.

[OpenAI Codex](https://github.com/openai/codex) 기반의 독립 작업으로 OpenAI 공식 제품이 아닙니다. [Apache-2.0](LICENSE) · [NOTICE](NOTICE).
