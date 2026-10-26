# OpenDIY Codex

**Unofficial Codex CLI & Rust App Server patches, diagnostics, and reproducible engineering experiments.**

> **Project status: Early development.** This repository is being initialized for public patch tracking. No patched binaries, installers, or verified releases have been published here yet.
>
> **Not an official OpenAI project.** OpenDIY Codex is an independent effort and is not affiliated with, endorsed by, or maintained by OpenAI.

[English](#english) · [한국어](#한국어)

## English

### Why OpenDIY Codex?

Codex is useful for software development, but diagnosing a difficult runtime issue often requires more than a workaround. OpenDIY Codex is a place to document failures, maintain independently developed changes to **open-source Codex components**, and publish the evidence needed to assess those changes.

The primary focus is **Windows reliability**, with attention to Rust App Server behavior, process lifecycles, observability, and developer experience.

### Areas of investigation

These are **investigation and development targets**, not claims that a fix has shipped:

- **Runtime stability:** child-process ownership and cleanup, graceful termination, cancellation, and long-running sessions.
- **Windows behavior:** socket and handle lifecycles, event-loop behavior, and process/resource accumulation.
- **Diagnostics and performance:** bounded, timestamped logs; latency analysis; and reproducible incident traces.
- **Session and configuration behavior:** correct isolation of per-session settings from global defaults.
- **Extension workflows:** feasibility of refreshing skill, plugin, and MCP configuration without losing active session context.
- **Regression evidence:** pinned upstream revisions, failing reproductions, platform-specific tests, and clear PASS / FAIL / NOT_RUN reporting.

The scope may change as investigations produce evidence. An issue being listed here does not mean it is an upstream-confirmed defect.

### What belongs in this repository?

| Material | Policy |
| --- | --- |
| Patches to Apache-2.0-licensed Codex CLI and Rust App Server source | In scope, subject to the upstream license and attribution requirements |
| Original diagnostic scripts, tests, and documentation | In scope, after removing credentials and private information |
| Reproduction cases and validation records | In scope, with explicit versions and limitations |
| Decompiled, extracted, or modified **proprietary desktop app** assets | **Not distributed here** without separate permission or a verified redistribution right |
| Unverified fixes presented as stable releases | **Not permitted** |

The [upstream Codex project](https://github.com/openai/codex) is the source of truth for official behavior and releases. Changes in this repository should be described as independent patches, not official fixes.

### Evidence-first patch policy

A published patch should identify:

1. **Baseline:** upstream tag/commit, component, platform, and relevant versions.
2. **Problem:** observable symptoms and the conditions required to reproduce them.
3. **Root cause:** the supported explanation, distinguishing established facts from hypotheses.
4. **Change:** modified files and why the change addresses the cause rather than only the symptom.
5. **Verification:** exact tests run, environment, observed results, known failures, and skipped checks.
6. **Compatibility and rollback:** applicable constraints and how to return to the official baseline.

Use **PASS**, **FAIL**, and **NOT_RUN** only for the checks they actually describe. A successful build does not establish that an entire runtime or desktop integration has been validated.

### Releases and installation

**There is currently no installable OpenDIY Codex release in this repository.** Do not treat the repository name or README as evidence that patched executables are available or production-ready.

When releases become available, this section will document the required baseline, build instructions, checksums, supported platforms, and known limitations.

For official Codex installation and documentation, use the [OpenAI Codex repository](https://github.com/openai/codex) and [official documentation](https://developers.openai.com/codex).

### Contributing and reporting issues

Reproduction reports are most useful when they include the upstream version/commit, operating system, expected versus observed behavior, concise steps, and sanitized logs. Please distinguish original upstream behavior from behavior introduced by a local patch.

Never attach credentials, API keys, authentication files, private conversation transcripts, personal paths containing sensitive data, or unredacted dumps.

### License, upstream attribution, and trademarks

Original materials published by this project are provided under the [Apache License 2.0](LICENSE), unless a specific file states otherwise. Apache-2.0-licensed material imported from [OpenAI Codex](https://github.com/openai/codex) remains subject to its original notices and license conditions. See [NOTICE](NOTICE).

This license does **not** grant rights to redistribute separately licensed or proprietary OpenAI desktop application code, assets, or installers. OpenAI and Codex names are used only to identify the upstream software; no endorsement is implied.

---

## 한국어

**OpenDIY Codex**는 공개된 Codex CLI 및 Rust App Server를 대상으로 결함 재현, 비공식 패치, 진단 도구, 검증 근거를 관리하는 독립 프로젝트입니다.

공식 OpenAI 프로젝트가 아니며, 현재 이 저장소에 배포 가능한 패치 실행 파일이나 안정화된 릴리스는 없습니다.

### 주요 개발·조사 영역

- **Windows 런타임 안정성:** 자식 프로세스 수명·정리, 소켓·핸들 관리, 장시간 실행 중 자원 누적.
- **관측 가능성 및 성능:** 재현 가능한 로그, 지연 측정, 장애 분석 자료.
- **세션 설정:** 세션별 설정과 전역 설정의 적용 범위 분리.
- **플러그인·스킬 연동:** 세션 문맥을 보존하면서 갱신하는 방식의 구현 가능성 검토.
- **회귀 검증:** 원본 버전 고정, 실패 조건 재현, 수정 전후 결과 및 미검증 항목 명시.

이 목록은 **개발·조사 계획**이지, 해당 기능이 모두 구현되거나 검증되었다는 선언이 아닙니다.

### 공개 원칙

수정 사항은 원본 커밋, 결함의 재현 조건, 원인, 패치 내용, 실제 실행한 검증과 미실행 항목을 구분해 공개합니다. **빌드 성공을 전체 기능 검증 성공으로 표현하지 않습니다.**

Apache-2.0으로 공개된 Codex 구성요소 및 직접 작성한 코드·테스트·문서를 중심으로 배포합니다. 별도 재배포 권한이 확인되지 않은 **비공개 데스크톱 앱의 원본·수정 바이너리·추출된 자산은 배포하지 않습니다.**

자세한 저작권 및 비공식 프로젝트 안내는 [LICENSE](LICENSE)와 [NOTICE](NOTICE)를 참고해 주세요.

---

*OpenDIY Codex — Fix what you can verify. Document what you cannot.*
