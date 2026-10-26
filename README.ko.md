# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

Windows에서 Codex를 사용할 때 생기는 요청 정체와 하위 프로세스 정리 문제를 수정하는 Codex CLI·Rust App Server 포크입니다.

현재 공개된 r22 패치는 MCP 요청 취소, 서버 종료와 대기, 설정 읽기 경로를 다룹니다.

## 바뀌는 점

- MCP 읽기 요청을 취소하면 그 요청이 직접 소유한 작업과 전용 연결·프로세스를 정리한 뒤 응답하도록 했습니다.
- 직접 소유한 MCP 프로세스가 실제로 종료될 때까지 정리를 이어가고, 이전 프로세스 정리가 끝난 뒤 재시도하도록 했습니다.
- 출력이 없는 MCP 서버를 기다릴 때 불필요하게 파일 I/O 스레드를 점유하지 않도록 했습니다.
- 불필요한 설정 재조회를 없애고, 설정 응답과 기능 설정이 같은 시점의 값을 사용하도록 했습니다.

## 소스와 문서

- [CLI 소스](codex-rs/cli) · [App Server 소스](codex-rs/app-server) · [소스 구조와 빌드 안내](docs/source-import.md)
- [수정 내역](MODIFICATIONS.md) · [테스트 방법과 한계](docs/TESTING.md)

[OpenAI Codex](https://github.com/openai/codex)를 기반으로 한 비공식 독립 프로젝트입니다. [Apache-2.0](LICENSE)과 구성요소별 [제3자 고지](NOTICE)를 따릅니다.

현재는 r22 소스만 제공하며, 설치 가능한 패치 바이너리와 비공개 데스크톱 UI 자산은 제공하지 않습니다. 데스크톱 앱 전체 검증과 메모리 누수·UI 지연의 완전한 해결은 확인하지 않았습니다.
