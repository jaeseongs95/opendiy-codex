# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

> Because "fuck it" wasn't enough. I fixed it.

Windows용 Codex CLI·Rust App Server의 요청 취소, 하위 프로세스 정리, 화면 이력 보유와 오류 종료 시 로그 유실을 개선합니다.

현재 소스는 **공식 0.165.0 기반 R29**입니다.

- **작업 정체 개선:** 한도가 있는 용량 부족 재시도, 취소 전달, 직접 소유한 프로세스 정리.
- **대화별 제어:** 컨텍스트 창 크기, 범위가 분리된 메모리, 하단 서브에이전트 상태 표시.
- **화면 부담 완화:** 공식 이력 조회 기능을 재사용하고, 모델에 전달할 원본 대화를 자르지 않으면서 화면 보유 구간을 제한.
- **장애 원인 추적:** 선택형 버퍼 로그, 완결 파일 단위 저장, 실패 후 재시도하는 비동기 내보내기. 주요 화면 한국어 지원.

[소스 빌드](docs/source-import.md) · [CLI 설정](docs/native-cli-r27.ko.md) · [R29 검증 상태](docs/TESTING.md) · [R28 검증과 한계](docs/cli-r28-validation.md) · [변경 내역](MODIFICATIONS.md)

데스크톱 화면은 [Agent Grid](https://github.com/jaeseongs95/agent-grid), CLI·App Server는 이 저장소에서 관리합니다. 실제 인증된 모바일 앱과 같은 백엔드에 연결하는 검증은 남아 있습니다. 프롬프트 캐시 적중률 향상이나 메모리·연결 누적·화면 지연의 완전한 해결을 주장하지 않습니다.

**소스와 빌드 도구만 공개합니다.** 설치용 실행 파일, 비공개 데스크톱 자산, 인증 정보와 운영 로그는 포함하지 않습니다. [OpenAI Codex](https://github.com/openai/codex) 기반 비공식 독립 프로젝트이며 [Apache-2.0](LICENSE)과 구성요소별 [제3자 고지](NOTICE)를 따릅니다.
