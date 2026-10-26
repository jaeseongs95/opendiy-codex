# OpenDIY Codex

[English](README.md) · [한국어](README.ko.md)

**수정은 소스로, 검증 범위는 분명하게.**

Codex CLI와 Rust App Server의 Windows 안정성 패치를 소스로 공개합니다.

## 무엇이 좋은가

- **수정 코드를 직접 봅니다.** Windows 프로세스·Job·stdio·소켓 수명과 MCP 요청 취소를 다룬 패치를 공개합니다.
- **무엇을 시험했는지 압니다.** r22 Windows 후보의 Cargo 표적 시험 18개 통과와 미검증 범위를 함께 설명합니다.
- **원본과 변경을 대조합니다.** 전체 Rust workspace와 고정된 upstream 버전, 소스 해시를 제공합니다.

## 살펴보기

- [CLI 소스](codex-rs/cli) · [Rust App Server 소스](codex-rs/app-server)
- [테스트 방법과 한계](docs/TESTING.md)
- [원본 버전](SOURCE_BASELINE.json) · [수정 내용](MODIFICATIONS.md)

과거 r22 시험 결과가 새 공개 clone의 release build나 전체 desktop 검증 완료를 뜻하지는 않습니다.

[OpenAI Codex](https://github.com/openai/codex)를 바탕으로 한 비공식 독립 프로젝트입니다. [Apache-2.0](LICENSE)과 해당 [제3자 고지](NOTICE)를 따릅니다.
소스만 공개합니다. 설치 가능한 패치 바이너리는 제공하지 않으며, 비공개 desktop 자산은 재배포하지 않습니다.
