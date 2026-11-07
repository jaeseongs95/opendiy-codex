# R39 compact 개발 후보 (2026-11-07)

정식 발행·동결된 R38과 같은 공식 0.167.0 기반에서 압축 동작만 변경합니다. 버전은 `0.167.0+native-r39.20261107`이며, 고정 소스는 `c4399dce957bb63fe1fdc276aa4306c06f6cf47f`입니다. 메모리 리팩토링·후크·캐싱·스크롤 감도·한글화·컨텍스트 조회의 미완 작업은 R40으로 이관하며 이번 변경에 포함하지 않습니다. 현재 소스·시험 후보이며 최종 빌드·발행은 미완료입니다.

- 완료된 user·hook·agent 원문을 압축 후 활성 문맥에 자동 재첨부하지 않습니다. 현재 미완 작업을 이어갈 요약 기준을 공유하며, 빈 로컬 압축 결과는 이전 답변으로 대체하지 않고 오류로 처리합니다.
- canonical context, 명시된 영속 client 제어, 백그라운드 snapshot 이후 입력과 저장 원본·승인 검토 근거는 보존합니다. 압축 요청 전용 지침에도 기존 메시지 ID 규칙을 적용합니다.
- 개별 실행 파일에 필요한 같은 릴리스의 `codex-code-mode-host.exe`를 별도 자산으로 제공할 준비와 정확한 동반 배치 안내를 추가합니다. 실제 자산 발행·다운로드 검증은 아직 수행하지 않았습니다.

현재 검증 상태는 [TESTING](docs/TESTING.md), 동작·패키지 조건은 [R39 안내](docs/native-cli-r39.ko.md)를 따릅니다. 아래는 이전 후보별 변경 이력입니다.

# R37 공개 소스 pin 후속 동기화

고정 소스는 `8a3b7d35a83467504c59448087f27c7a5c0e0e9b`, 이전 공개 export 기준은 `8c0d816ec3c9256a02334c81a5e965b977ac187a`입니다. 이전 제품 후보 `89139fd7386eaebd9d0cff6ffccb8dc93c2859ea`에서 달라진 파일은 `codex-rs/cli/tests/ui_language.rs` 1개이며 제품 동작 변경은 없습니다. 기본값 false의 disable 키 제거 계약에 맞춰 오래된 시험 assertion을 바로잡았습니다. 이 공개 작업에서는 Cargo·컴파일·실행·설치를 수행하지 않습니다.

R36 정식 발행·동결과 R37 개발 후보 상태를 유지합니다. 공동시험·수용·전체 독립감사·최종 릴리스는 미완료입니다. 기존 운영 관측 문서 제외, 합성 사례 문서 8개와 단일 Python 생성 wire API, 공식 patch·inventory·프롬프트 38개 계약을 유지합니다. 이번 공개 소스 검증의 명령·원시 출력·종료 코드는 별도 외부 기록 `public-source-verification-02`에 보존하며 아래 이전 후보의 검사 결과와 구분합니다.

## 이전 89139fd 공개 소스 기록

# R37 공개 소스 개발 후보

고정 제품 소스는 `89139fd7386eaebd9d0cff6ffccb8dc93c2859ea`, 이전 공개 기준은 `f04304b239491311e310ca86a2a812d138002917`입니다. 공식 0.166.0 기반과 기존 R36 변경을 유지하며 메모리 상태·동시 요청, daemon 실행, 서브에이전트 선택·화면 재생, 설정, 플러그인 캐시의 후속 변경을 포함합니다. 소스 전달용 개발 후보이며 최종 릴리스·설치 완료를 뜻하지 않습니다.

고정 Git blob만 내보내며 R37 사례 문서 8개와 변경된 Python 생성 API 모듈 1개를 명시적으로 포함합니다. Python SDK 전체 배포를 새로 구성하지 않으며 변경된 wire 타입만 제공합니다. 운영 설정 관측이 섞인 `docs/r37-cache-observation-status.ko.md`는 파일과 patch에서 공개 제외합니다. 원시 로그·로컬 시험 결과물·실행 파일·인증·운영 데이터는 내보내지 않습니다. 원본 고지, 공식 대비 patch, inventory, SOURCE_BASELINE과 내장 프롬프트 38개의 바이트 해시 계약을 유지합니다. 이번 공개 작업 사본에서 Cargo·컴파일·실행·설치는 수행하지 않습니다. 제품 시험 결과는 각 사례 문서의 범위를 따르며 공개 소스 검증으로 확대하지 않습니다.

## 이전 R36 후보 기록

# R36 개발 후보

고정 소스는 `284253eff61a6a0e2976d2f9eee5f9a841aad8d5`입니다. PR30은 사용자가 병합했으며 후속 변경은 PR31에서 검토합니다. 제품 빌드·동결·main 병합·릴리스·설치는 하지 않습니다.

1992480a의 thread-store 293개·Core cleanup 36개가 통과했습니다. 이후 변경은 CLI의 한 시험에 한정되며 제품·비CLI 소스는 동일합니다. 기존 34개 정리 시험을 유지하고 실제 Local owner의 삭제 전후 잠금·취소·실패 2개를 추가했습니다. 제품 cold/unloaded 정리는 참조 참여자를 입증할 수 없어 `reference_participants_unverified`로 보존하며 운영 지원은 미완입니다.

현재 후보 CLI 시험은 localization 15개, root parser 1개, CI 8개, 최초 설치 parser 1개가 통과했습니다. CI의 subprocess timeout 검사용 child fixture 1개는 직접 실행 시 ignored이며 통과로 계산하지 않습니다. 해당 fixture는 timeout 상위 시험에서 별도 프로세스로 실행합니다. 실제 GitHub 네트워크·인증 검증은 이 단위 시험 범위에 없습니다. 기존 한국어 전역 선언의 영문 도움말 기대값을 명시하고 Clap 상속 전에 설정해 나머지 출력의 완전 일치를 검사합니다. 선행 후보에서 발견한 실제 cleanup long_about 연결 누락과 마침표 매핑도 보완했습니다.

서브에이전트 제한 높이 표시·Cloud/읽기 전용 CI 관측·fast 전파·내용 없는 compact 계측을 포함합니다. 나머지 후보 검증과 fresh 독립감사는 진행 중이며 전체 완료를 주장하지 않습니다. R34 발행본·R35 원본·공유 daemon·사용자 데이터를 보존합니다. 소스 9454개/공식 대비 변경 717개를 내보내며 원시 로그·실행 파일·인증은 게시하지 않습니다.

## 이전 후보 기록

# R35 통합 개발 후보

고정 소스는 `2e9f6050aac19bb230dfcff10138d255edc613f7`, 이전 공개 기준은 `28e34926069af7a647f07d3ee013fb4137bb0107`입니다. 공식 0.166.0 기반을 유지하며 R34 발행본은 동결합니다.

메모리 명시 활성화와 세션 설정·compact 처리, 전역 진단 설정, i18n 키 조회, 원자 최초 설치와 버전 문자열만으로 공유 서버를 거절하던 경로를 보완했습니다. 소스 9,439개와 공식 대비 변경 682개를 내보냈으며 내장 프롬프트 38개의 바이트 조건과 수정 고지를 검사합니다. SSE usage 계측 수정은 서버 캐시 정책 변경이나 실제 적중률 개선의 입증이 아닙니다.

공개 출처 검사 종료 0을 확인했습니다. 앞선 후보의 통합 시험 컴파일은 종료 0이었지만 행동시험 7개 필터가 실패했습니다. 이 후보에서 Windows 설정 교체와 fixture·번역 키를 보완했으며, 후속 시험 컴파일은 사용자 디스크 부족 중단 요청으로 종료 130·행동시험 0건입니다. 현재 수정본의 컴파일·실행 통과는 미확인입니다. 최종 실행본·Desktop 공동시험·최종 독립감사·릴리스·설치는 아직 완료되지 않았습니다. 이전 후보의 결과를 현재 전체 후보의 통과로 해석하지 않습니다. 실행 파일과 인증·운영 로그는 Git 소스에 포함하지 않습니다.

## 이전 후보 기록

# R35 개발 중 소스 — i18n 카탈로그와 초기화 capability

고정 소스는 `0133cf002d1e1215e71292e004ed8d56cadba9dc`, 이전 공개 내보내기는 `40e952e5211ab2059de7250bbbec44b211aac19c`, R34 비교 기준은 `0f4a2d3062f2fd9bc234c539f3c7e8f0b2add274`다. 공식 0.166.0 기반 `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856`를 유지한다. R34 실행본·기존 공개 PR은 보존하며 R35는 아직 동결 전인 개발 후보다.

R34 고정 소스 대비 145경로에 영어·한국어 Fluent 카탈로그, CLI·TUI·cloud 화면 문구와 언어 선택, app-server 초기화 capability·호환성 시험 소스를 반영했다. 기존 수정 고지는 유지하고 이번 변경 경로에만 R35 고지를 연결한다. 카탈로그 같은 데이터 파일의 변경은 `SOURCE_BASELINE.json`·`upstream/source-inventory.json`과 공식 대비 패치에 기록한다.

이번 공개 checkout의 기존 Python 출처 검증은 소스 9,431개·공식 대비 수정 670개·내장 프롬프트 38개에서 종료 0이었다. 내보내기 도구 시험은 19개 중 18개 통과·실제 심볼릭 링크 생성 1개 건너뜀으로 종료 0이었다. 전체 Cargo 빌드·Rust 제품 시험, 스키마 재생성, CLI·TUI 실행·화면 확인·부하 검증은 수행하지 않았다. 이 소스 PR은 실제 사용 효과 검증이나 릴리스·설치 승인을 뜻하지 않는다. 재현은 `python -B scripts/source-import/verify.py`와 `python -B -m unittest discover -s scripts/source-import -p test_verify.py`로 한다. 원시 로그·운영 근거·실행 파일·인증·로컬 설정은 포함하지 않는다.

## 이전 후보 기록

아래는 각 이전 후보의 당시 기록이며 현재 R35 전체의 검증 결과가 아니다.

# R33 graceful 완료 통지 경계 수정 후보

현재 고정 소스는 `36ae395a552fd55962e19409cbf29b1fdf03b0ec`, 이전 공개 기준은 `5300c7238a23cd29d29e4ffdfbcaf3ac53366886`다. 이전 `3d9` 후보에서 달라진 제품 소스는 `bespoke_event_handling.rs`, `outgoing_message.rs`, `thread_status.rs`의 3경로다. 완료·중단 알림의 쓰기 확인이 끝날 때까지 drain 책임을 유지하고 기존 idle 알림 순서를 보존하는 후속 개발 후보다. 제품 버전은 `0.166.0+native-r33.20261102`를 유지한다.

개발용 시험 컴파일은 종료 코드 0으로 완료했고 표적 시험 34개가 중복 없이 통과했다. 개발용 서버 컴파일은 진행 중이며 새 후보의 runtime·Desktop 공동 시험·최종 독립감사는 미완료다. 새 release 빌드·패키지 생성은 시작하지 않았다. 이전 `3d9` package03의 graceful 완료 통지 누락과 최종 독립감사 FAIL 기록은 그대로 보존하며 전체 수용·릴리스·설치를 보류한다. 이전 후보의 시험·빌드·부분 Desktop 공동 시험 통과를 새 후보의 통과나 결함 해소로 확대하지 않는다.

source 9,412개·공식 대비 수정 558개를 고정 Git blob에서 수집한다. 기존 R32 기본 고지와 R33 개별 고지 51경로를 보존하고 이번 3경로를 더해 R33 고지는 총 54경로다. 원본 Apache 고지·LICENSE·NOTICE와 내장 프롬프트 38개의 원본·Windows 바이트 해시는 유지한다. 프롬프트 catalog의 source commit만 현재 후보에 연결한다. 원시 로그·실행 파일·개인 경로는 공개하지 않는다. [후보별 검증 범위](docs/r33-development-verification.ko.md)를 따른다.

## 이전 3d9 후보 기록

고정 소스는 `3d9eb86445925caa920dd2f958496cf1aa4ad7e2`, 이전 공개 기준은 `b1a8780e283c3f3514387ec209ded2d78c6e91b7`이다. 제품 소스 버전은 `0.166.0+native-r33.20261102`를 유지한다. 같은 후보의 개발용 시험 컴파일은 종료 0이며 daemon 전체 66개 통과·0개 실패·native fixture 1개 무시, nested stdio 개별 재실행 1개 통과를 확인했다. 최적화 CLI·앱 서버·launcher 빌드는 종료 0이었고 package03 생성·해시 결속도 확인했다. 자연 EOF 8경계·격리 GUI 복원 범위의 Desktop 공동 시험은 통과했으나 graceful 완료 통지 실패로 전체 수용·릴리스·설치를 보류한다. 최종 독립감사는 이 차단 결함으로 FAIL이다. 실행 패키지 공개·실제 예약 등록·모바일·장기 부하는 미검증이다.

앞선 package02의 중첩 launcher에서 분리한 하위 프로세스가 PIPE 핸들을 상속해 호출자의 EOF를 지연시키는 결함이 확인됐다. 이번 소스는 Windows 공통 spawn 경로에서 `HANDLE_LIST`로 전달할 표준 핸들을 제한하고 관련 PID·분리 실행 fixture를 보강한다. `codex-rs/app-server-daemon`의 7경로가 바뀌며 새 `src/backend/windows_spawn.rs`를 포함한다. 개발용 회귀시험과 빌드·패키지 준비 성공을 실제 runtime에서 결함을 해결했다는 보증으로 해석하지 않는다.

공개 source는 9,412개·공식 대비 수정 557개다. 기존 R32 기본 고지와 R33 고지 46경로를 유지하고 추가 5경로에도 R33 개별 수정 고지를 연결한다. 원본 Apache 고지·LICENSE·NOTICE와 내장 프롬프트 38개의 원본·Windows 해시를 보존한다. fixture의 실행 파일·원시 로그·개인 경로는 포함하지 않는다. [후보별 검증 범위](docs/r33-development-verification.ko.md)를 따른다.

## 이전 0c588 후보 기록

아래는 이전 후보의 기록이며 새 핸들 경계 수정 후보의 검증 결과가 아니다.

# 공식 0.166.0 기반 R33 검증 중 소스 — 다음 CLI 선택·서버 보존

현재 고정 소스는 `0c588be6c85098f0ff139ec7a9bc119c1da71f20`, 이전 공개 기준은 `8f52cad14bf45e76ef123aeea9f7fcfa22ac3b73`이다. 전체 제품 소스 버전은 `0.166.0+native-r33.20261102`로 식별했다. 같은 소스의 개발용 시험 컴파일은 종료 0이고 표적 Rust 시험은 19그룹 73개 통과·0개 실패였다. 최종 제품 실행 파일·설치·릴리스·실제 사용 환경의 검증은 미완료다.

Windows 자동업데이트용 PowerShell·숨김 VBS·격리 시험·안내문과 Rust launcher를 포함한다. 검증한 패키지의 포인터를 원자 교체해 다음 CLI 실행이 새 패키지를 선택하도록 작성했으며, 기존 패키지와 공유 daemon은 보존한다. 자동 CLI 선택을 공유 서버 전환 성공으로 간주하지 않는다. session cleanup 명령의 backend 선택을 공통 시작 경로로 맞추고 암시적 로컬 서버 연결에는 CLI·서버의 전체 버전 일치를 요구하도록 작성했다. 명시적 원격 연결을 이 규칙으로 자동 전환하지 않는다.

비강제 종료 재요청이 drain을 강제 종료로 승격시키던 F2의 수정 소스를 포함한다. 같은 후보에서 반복 graceful-only 요청의 drain 보존 회귀시험이 통과했다. Windows의 구 R28/R32 서버는 새 graceful-only 요청을 거절하며 강제 가능 요청으로 fallback하지 않도록 작성했다. 이 소스 공개와 합성 시험 통과는 배포·설치 승인이나 실제 사용 환경 검증을 뜻하지 않는다.

현재 후보와 바이트가 같은 helper·패키징 도구의 PowerShell 격리 13개와 패키징 13개 시험은 종료 코드 0이었고, `update.lock` 배타 lease 보강 후 전달 준비 21개 fixture도 통과했다. helper는 실제 예약 상태를 읽기 전용으로 표시하고 전체 Cargo 버전·릴리스 태그·필수 receipt 경로와 launcher 호환성 메타데이터를 검사하도록 보완했다. 실제 예약 작업 등록·운영 네트워크·실제 실행 파일·바로가기 설치·daemon runtime은 이 결과에 포함하지 않는다. 앞선 재생·캐시 시험은 각 이전 후보의 근거로 구분했다. [후보별 검증과 제한](docs/r33-development-verification.ko.md)을 따른다.

기존 R32 기본 고지·공식 pin·프롬프트 입력 38개는 유지하고 추가 source/tools를 고정 Git blob·inventory·공식 대비 patch에 결속한다. 새 Rust launcher와 `tools/windows-cli`의 updater·시험·안내문을 포함하며, 실행 파일·원시 로그·개인 경로·비밀은 공개하지 않는다.

## 이전 76c 개발 후보 기록

아래는 당시 소스와 검증의 기록이며 현재 후보 전체의 검증 결과가 아니다.

# 공식 0.166.0 기반 R33 개발 중 소스 — 비강제 갱신·상태 조회·캐시 관측

현재 고정 소스는 `76c0a7ab3b971918cd3d9f30daf8e1344941359c`, 이전 공개 기준은 `5537d7bf4f0730969dfaf1c1ed876acb8920a5e1`다. 전체 소스의 Cargo 버전은 아직 `0.166.0+native-r32.20261101`이며, R33 제품 버전 스탬프·전체 실행 파일·설치·릴리스는 완료하지 않았다. 이번 공개는 개발 중 소스와 제한된 검증 요약이다.

비강제 전환은 drain 중 재요청 시 강제 종료로 승격되는 결함(F2)이 확인되어 후속 수정 전 배포 불가 상태다. 아래 캐시·메시지 재생 시험 결과는 각 후보의 해당 범위에서 유효하지만 이 결함을 해소한 근거는 아니다.

앞선 재생 수정에 `update --from-cli --no-force`, 수동 상태 조회 `status --json`, 캐시 관측의 명시적 설정 선택과 관련 시험을 더했다. 비강제 갱신은 updater 종료 요청을 보내지 않고 기존 installer 락을 확인하는 보완을 포함한다. 상태 조회의 `stopped`는 비원자 관측값이며 갱신 안전 판정으로 쓰지 않는다. 캐시 관측은 일반 logging과 별도 파일의 `response=true`, `wire=true` 선택으로 켤 수 있고 기존 환경변수를 추가로 요구하지 않는다. 이 소스 공개 과정에서 운영 설정이나 실행 중 프로세스를 변경하지 않았다.

원본 `278f41f` 이후 export 대상 델타는 새 캐시 관측 문서를 포함한 13경로다. 기존 재생 3파일의 R33 고지를 보존하고 새 델타 13경로만 R33 고지에 추가했다. 기본 R32 고지와 다른 소스 바이트는 유지했다. `docs/native-cli-r33-cache-observation.ko.md`를 명시적 source prefix에 추가하고 고정 Git blob·inventory·공식 대비 patch를 다시 결속했다.

검증 결과는 후보별로 구분한다. 재생 33개 통과는 `278f41f`, 보완 전 상태 조회 4개 통과는 `1b23c21c9`의 기록이다. 캐시 진단 23개 통과·2개 무시와 격리 자식 1개 통과, HTTP/WS 경계 부모 1개·격리 조합 6개 통과는 현재 `76c0a7ab3`의 기록이다. 비강제 갱신 보완 `970171b`의 앞선 정적 재검토에서는 새 차단 결함 0건이었으나 이후 독립 검토에서 F2가 확인됐다. 최종 후보의 동적 검증·최종 통합 CLI/daemon runtime·실제 사용 환경 검증은 미완료다. [후보별 검증과 제한](docs/r33-development-verification.ko.md)을 따른다.

공개 출처 검증은 소스 9,403개·공식 대비 수정 533개·내장 프롬프트 38개에서 종료 코드 0이었다. 도구 시험은 17개 통과·실제 심볼릭 링크 생성 1개 건너뜀이다. 원시 로그·개인 경로·인증 자료·실행 파일은 추가하지 않는다.

## 이전 R33 메시지 재생 후속 소스 기록

아래는 당시 후보의 기록이며 현재 개발 후보 전체의 검증 결과가 아니다.

# 공식 0.166.0 기반 R33 메시지 재생 후속 소스

고정 소스는 `278f41f74f26b72cd2f3bf068400d8454a00ff75`, 공개 기준은 `3f2185d69aa14ff8cb9f0991f119bdc6f222de7e`다. 기존 R32 소스에 메시지 재생 관련 3파일만 더한 공개 소스 후속 변경이며, Cargo 버전은 `0.166.0+native-r32.20261101`을 유지한다. 자동 갱신 구현이나 새 실행 파일 배포를 포함하지 않는다.

완료 이력과 버퍼에 겹친 동일 thread·turn·item ID 및 payload의 사용자·에이전트 메시지가 다시 표시되지 않도록 재생 사본을 정리한다. 질문·완료 이벤트·권한 요청과 다른 ID의 동일 본문, 새 item, 미완료 델타를 보존한다. 이력에 포함된 완료 본문만 억제하며 원본 버퍼를 수정하지 않는다.

고정 원본 소스의 관련 시험은 4개 실행에서 33개 통과·0개 실패였다. 공개 소스의 출처 검사는 9,401개·수정 526개·내장 프롬프트 38개에서 종료 코드 0이었고, 도구 회귀시험은 17개 통과·실제 심볼릭 링크 생성 1개 건너뜀이다. 공개 checkout의 Rust 빌드·실제 앱 효과·설치·릴리스는 미검증이다. 후보·시험·한계는 [R33 검증 요약](docs/r33-replay-verification.ko.md)에 구분했다.

`SOURCE_BASELINE.json`·`upstream/source-inventory.json`·`upstream/official-0.166.0.patch`를 현재 고정 소스로 재생성했다. 기존 R32 고지는 유지하고 이번 3경로만 R33 고지를 사용한다. `scripts/source-import/export.py`의 기본 고지는 R32이며 `NOTICE_OVERRIDES`가 세 경로의 R33 고지를 선택한다. 원시 로그·개인 경로·인증 자료·실행 파일은 추가하지 않는다.

## 이전 R32 후보 이력

아래는 당시 후보의 기록이며 현재 R33 후속 소스 전체의 완료 근거가 아니다.

# 공식 0.166.0 기반 R32 후보

고정 소스는 `c23bf2aaed0c31d147ce915d5478c926282a5482`, 버전은 `0.166.0+native-r32.20261101`이다. R31의 별도 릴리스는 취소하고 기존 통합 수정은 R32에 포함한다.

공용 로그 설정을 유지한 채 별도 캐시 진단 설정을 추가했다. 기존 응답·송신 진단의 필터와 자료형 누락을 수정하고, 실행 식별자·시작 시 확정한 모드를 배치에 결속했다. 실제 로컬 HTTP·WebSocket 요청부터 응답 해석·완결 파일까지 확인하는 시험을 추가했다. 시험 작성과 실행 통과는 구분하며 최종 실행·공동시험·감사·설치·릴리스는 미완료다. 캐시 정책 변경이나 적중률 개선이 입증된 상태가 아니다.

공개 소스 9,401개·수정 524개·내장 프롬프트 38개의 출처 검사는 종료 0이었다. 원시 로그·설정·자격 증명·빌드 실행 파일은 소스 저장소에 포함하지 않는다. 공식 갱신·공유 서버 전환·복구 설계는 `docs/native-cli-r32-upgrade-design.ko.md`에 있으며 새 자동 갱신 구현은 포함하지 않는다.

## 이전 R31 후보 이력

아래는 당시 후보의 기록이며 현재 R32의 완료 근거가 아니다.

# 공식 0.166.0 기반 R31 소스 후보

공식 태그 `rust-v0.166.0`의 커밋 `6ff1e1ca3b30fc8b914720681fcf5fe62e95f856` 전체를 기반으로 한다. 현재 고정 소스는 `4f63b43b1c7eeffa09318d4a0a5ecb8ff6bd9f31`, 버전은 `0.166.0+native-r31.20261101`이다. R30의 선택 이식과 구분한다.

기존 대화에 연결 수명 동안만 도구를 보강하는 경로, 종료된 실행기의 입력 거절과 정리, 대화 조회의 큰 본문 바이트 탐색, 연결 정리 결과 보존을 수정했다. 비영속 도구 보강의 잠금 순환을 제거했고 실제 사용자 응답·전달 기록이 겹치는 회귀시험을 추가했다. 예약 안내·상태·오류는 전역 언어 선택을 따르며 명령·사용자 문구·서버 사유는 보존한다. 응답 자료형과 생성 스키마도 함께 갱신했다.

기존 공개 main `f28920f3761bce6d51f2ceb21d69a3473e7ccfee`에만 있던 실제 호스트 정리 시험 7개와 시험 지원을 보존했다. 시험 지원은 명시적인 `test-support` 기능으로만 활성화한다.

이 후보는 검증 중이다. 시험용 생성자 필드 누락 2건, TUI 통합 오류 13건과 코어 시험의 이전 실행 요청·출력 자료형 참조 23건을 교정했다. 공식 시험 환경과 동일한 스택 8 MiB에서 화면 표적 시험은 이전 후보 9cce49e8f 기준 14개 통과·1개 실패였다. 실패 원인은 일부 인자 없는 로컬 제어 명령이 추가 필드를 무시하는 동작이며, 현재 후보는 공통 입력 경계에서 이를 거부하도록 수정했다. 수정 후 재컴파일·실행은 진행 중이다. 전체 회귀시험·제품 빌드·공동 연동·설치·릴리스는 완료되지 않았다.

공개 내보내기 검사에서는 소스 9,399개, 수정 520개, 작업공간 구성원 165개, 내장 프롬프트 입력 38개의 출처·바이트를 확인했다. 수출 도구 회귀시험은 15개 통과, 실제 심볼릭 링크 생성 시험 1개는 이 Windows 환경에서 건너뛰었다. 이 검사는 Rust 컴파일·실행 검증을 대신하지 않는다.

새 변경 목록과 전체 고정 입력은 `SOURCE_BASELINE.json`·`upstream/source-inventory.json`, 공식 대비 패치는 `upstream/official-0.166.0.patch`에 있다. 소스·빌드 도구만 포함하며 기존 LICENSE·NOTICE·README 두 파일은 보존했다.

## 이전 0.165.0 기반 소스 이력

아래는 각 이전 후보를 기록한 당시의 설명이며 현재 R31의 상태가 아니다.

# Official 0.165.0 local modifications

OpenDIY Codex is independent and unofficial. This source snapshot is based on upstream release `rust-v0.165.0`, commit `360319d8dee62d87e2d59fc0c21805d79519cfed`, with local changes frozen at `57957241e90439377f4e34afbec79a6422f5872c`. The source version is `0.165.0+native-r30.20261030`. Upstream LICENSE, NOTICE and third-party notices are retained.

## R30 고정 후보 — 검증 중

R30 후속 고정 소스 `57957241e90439377f4e34afbec79a6422f5872c`는 공식 0.165.0 기반 R29·R30 위에 TUI 이력 표시 수정을 더한 후보이며, 버전은 `0.165.0+native-r30.20261030`이다. 공식 0.166 전체 기반으로 전환한 소스가 아니다. 이전 `86cf681e4` 대비 TUI 12파일이 바뀌었고, 공식 0.165.0 대비 수정 목록은 451파일이다. 성공한 대화 재개 뒤 아직 저장되지 않은 이력을 실패나 빈 완료로 확정하지 않도록 `Deferred` 상태와 저장 대기 안내를 분리했다. 불완전 이력·활성 입력의 보호를 유지하고, 정상 저장 대기에는 오류 셀이나 근거 없는 재시도 안내를 만들지 않도록 소스를 수정했다. 일반 오류와 `unknown`·`deleted` 사유는 기존 오류 경로로 남긴다. 새 안내는 기존 `i18n::text`와 사전을 사용해 기본 영어와 전역 한국어 선택에 연동한다. 지정 커밋 이후의 변경과 실시간 작업트리의 미커밋 변경은 포함하지 않는다.

이번 후보에는 시험 전용 `089257a53`의 이력 복원 회귀시험, `7a7ed5fa`의 제품 수정, `57957241`의 언어 연동과 실제 footer·검색 안내 소비 회귀시험 소스가 포함된다. 영어·한국어 선택별 일반·좁은 화면 안내와 검색 원문 미번역을 단언하는 시험을 추가했다. 이 후보의 Cargo 컴파일·회귀시험과 새 최적화 제품 빌드·실행 검증은 미실행이다. 시험 소스 추가를 시험 통과로 해석하지 않으며, 이전 `86cf681e4`의 6,307회 및 추가 시험 결과를 새 후보 전체의 통과 근거로 사용하지 않는다. 공개 내보내기 검증은 고정 Git 객체와 파일·프롬프트 바이트의 대응, 허용 범위와 민감정보 검사에 한정한다.

### 이전 86cf681e4 검증 이력

아래의 실행 파일·시험 결과와 완료·미완료 상태는 이전 `86cf681e4` 후보를 문서화한 시점의 기록이다. 현재 후보의 빌드·시험·설치 근거가 아니다.

이전 고정 소스 `86cf681e4`의 `compile13`과 후속 검증·빌드 단계는 종료 코드 0이었다. 같은 소스의 초기 5그룹과 후속 10그룹 결과 파일에는 합계 15그룹·6,307회 통과·0회 실패·5회 무시가 기록됐고, 각 그룹의 종료 코드는 0이었다. 이 가운데 전체 TUI 시험은 5,859개 통과·4개 무시, 전체 CLI 시험은 334개 통과·1개 무시였다. 합계는 기록된 실행 횟수이며, 필터 겹침을 제거한 고유 시험 수나 전체 작업공간의 모든 시험을 통과했다는 뜻은 아니다.

같은 고정 소스의 최적화 CLI 빌드는 종료 코드 0이었다. 최종 CLI 식별 자료와 검증 영수증의 버전은 `codex-cli 0.165.0+native-r30.20261030`, 실행 파일 SHA-256은 `1954507fdaaf0ecdd568725853b0735d20d426cbf97cb09bcd6ac5f592ddef21`로 일치하며, 검증 영수증의 종료 코드는 0이었다. 이 SHA의 CLI를 사용한 격리 실행 요약에서는 `retry`가 요청 2회·종료 0, `partial-no-replay`와 `hint-exceeds-window`가 각각 요청 1회·종료 1이었고 시간 초과는 없었다. 뒤의 두 종료 1은 요약에 명시된 오류 종료 경계의 관측값이며, 추가 응답 내용이나 전체 기능 통과를 뜻하지 않는다.

앱 서버의 최적화 실행 파일은 별도로 빌드 중이며 아직 완료·설치되지 않았다. CLI와 앱 서버를 합친 최종 독립감사·설치·공개 Release·실제 앱의 장시간 부하·모바일 검증은 완료 근거가 없다. 공개 소스 사본에서 새로 빌드하거나 실행한 결과가 아니며, 원시 시험 로그·실행 파일·개인 경로·인증 정보는 공개하지 않는다. 이전 후보의 부분 실패와 보호 중단은 [시험 이력](docs/TESTING.md)에 구분해 남긴다.

이전 R30 부분시험 이력(각 후보 당시의 상태): `1253c2014`의 통합 시험 컴파일은 TUI 시험의 비공개 필드 접근 2건으로 종료 코드 101이었다. 같은 컴파일에서 생성된 실행 파일로 스키마 시험 4개 통과·0개 실패·1개 무시와 CLI 정리 인자 시험 1개 통과·0개 실패를 확인했다. `bc1db4b14`는 해당 TUI 시험을 공개 입력 경계로 바꾼 후속 수정이며 이 소스 갱신 시점에는 재컴파일 전이다. `6855f4fd3`의 전역 언어 변경, `f120854c8`의 MCP 정리 경계 시험, 정리 수명 동적 시험, 후속 공동 입력 인터페이스·최종 독립감사·설치·모바일 검증은 미완료다. 앞선 R29 결과를 R30의 통과 근거로 재사용하지 않으며, 공개 사본의 제품 빌드·실행 결과를 주장하지 않는다.

## R29 선별 이식 이력

R29 고정 소스 `af098cef617d62fdfce51d5e1fbd0afb4533f75d`는 공식 0.165.0 기반에 공식 0.166 변경 5개만 선별 이식한 후보다. 전체 0.166 기반으로 전환하지 않았으며 버전 표기는 `0.165.0+native-r29.20261030`이다. 이전 `67404f4` 이후 34파일이 바뀌었고 공식 0.165.0 대비 수정 목록은 394파일이다. 제품 로직의 마지막 변경은 `159017bd9`이며, 이후에는 `3fdd3c3`의 Windows 알림 시험과 `af098ce`의 버전 표기만 바뀌었다. `159017bd9`에서 단위시험 11그룹 802개 통과·0개 실패·프로토콜 무시 1개와 표적 통합시험 5개 통과·0개 실패를 확인했다. `3fdd3c3`에서 Windows 알림 경계 시험 1개 통과·0개 실패·무시 0개와 실행 전후 원본 입력 불변을 확인했다. 최종 `af098ce`에서는 버전 표기 168곳 치환과 오프라인 잠금 메타데이터 검사 종료 코드 0을 확인했다. 이 결과는 808개 시험을 최종 커밋에서 전량 재실행했다는 뜻이 아니다. 이번 소스 갱신 시점에는 최적화 빌드가 진행 중이며, 새 실행 파일의 검증·제품 독립감사·설치는 미완료다. 공개 사본의 새 빌드·실행 결과나 실제 캐시율·장기 부하 개선 측정 결과는 아니다.

| 공식 변경 | 로컬 이식 | 범위 |
| --- | --- | --- |
| `eb3572f9d` | `b7151747a` | 도구 제안에서 불필요한 연결 도구 디렉터리 작업 감소 |
| `d93071fae` | `b900a85c4` | HTTP 응답 스트림에서 불필요한 출력 항목 보유 감소 |
| `f37aa4b1c` | `ce697e7b8` | 설정 처리의 불필요한 작업 감소 |
| `04e365c69` | `6a405165c` | 스트림 준비 뒤 샘플링 프롬프트 보유 해제 |
| `c62e9df39` | `159017bd9` | 재개·분기 때 추론 토큰 회계 보존 |

`upstream/official-0.165.0.patch` contains only the selected public source changes. `SOURCE_BASELINE.json` and `upstream/source-inventory.json` identify all 451 modified source/build-tool files and every exported Git blob. No local Git history is imported. The existing project README prose is retained with one current-source link added to each language; root LICENSE/NOTICE remain byte-for-byte unchanged. The r22 description is historical. This document and SOURCE_BASELINE identify the current source snapshot.

The changes cover MCP request cancellation and process ownership/cleanup, configuration and reload boundaries, capacity retry/trace ownership, background terminal startup yielding, cache observation, scheduled TUI work, terminal controls and related regression tests. `tools/windows-cli` contains packaging source and its disposable test fixture. This is a source export, not an installable desktop release or a claim that live-app latency, resource growth or all regressions are resolved.

아래 예약 도구 연결·설치 기록은 이전 고정 소스 `67404f4`의 이력입니다. 새 R29 후보의 설치·실행 결과가 아닙니다. 기존 R28 실행 파일은 보존하고 사용자 승인 후 새 패키지의 명령줄 바로가기·기본 공유 서버로 전환했습니다. 실제 모델이 자연어 요청에서 도구를 선택하는 평가는 실행하지 않았습니다. 기존 예약 실행기를 재사용하고 취소·중복 전달 경계를 보강하는 변경과 사용 안내는 [CLI 예약 실행](docs/native-cli-cron.ko.md)에 정리했습니다. 이 설치 전환을 기능 전체의 검증·운영 배포 완료로 해석하면 안 됩니다.

The root .gitattributes preserves the public repository's existing rules and merges the pinned source rules for generated schemas and LF source/build inputs; checks/.gitattributes is excluded. This merged source input is included in the 451-file modification/provenance list.

Modified Rust, TOML/lock, Python and tool configuration files carry a dated OpenDIY modification comment. Markdown notices are limited to documentation under `docs/`; runtime prompt templates keep their exact source bytes. `sourceSha256` records the pinned Git blob; `publishedSha256` records canonical published Git bytes, including those comments and the root attributes merge. The published bytes therefore differ from the private build input and are not claimed to reproduce a previously built executable. Inventory hashes describe committed Git bytes, not every physical Windows checkout byte. For the 38 recorded embedded prompt inputs, `upstream/windows-prompt-inputs.json` separately records canonical Git-blob SHA-256 and the pinned Windows source-input SHA-256. Only those paths override the existing `* -text` rule with native `text` EOL handling; Linux is not forced to CRLF. On Windows, the verifier checks the actual raw prompt bytes against the Windows hashes before comparing normalized Git bytes. These 38 inputs match the pinned private source input; this is not a whole-binary reproducibility claim. For generated TypeScript/JSON, compressed schema resources and snapshots, the data remains byte-exact and the following list is the external modification notice. Generated TypeScript must keep the actual GENERATED_TS_HEADER at byte zero: the original Rust schema comparison strips that exact prefix and compares the remaining serialized content. A prepended local notice breaks this contract even though it is a valid TypeScript comment. The exporter therefore leaves generated TypeScript exactly equal to the pinned source, without weakening the Rust comparison. Other data formats/snapshots also retain their original byte representation:

- `codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadSubscribePassiveParams.ts`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadSubscribePassiveParams.json`
- `codex-rs/app-server-protocol/schema/typescript/v2/CompletedChildCleanupResult.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/CompletedChildSnapshot.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadCleanupCompletedParams.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadCleanupCompletedResponse.ts`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadCleanupCompletedParams.json`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadCleanupCompletedResponse.json`
- `codex-rs/core/config.schema.json`
- `codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshParams.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshResponse.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadPluginReloadParams.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadPluginReloadResponse.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/TurnStartIfIdleParams.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/TurnStartIfIdleResponse.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/index.ts`
- `codex-rs/app-server-protocol/schema/json/ClientRequest.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`
- `codex-rs/app-server-protocol/schema/json/v2/McpServerRefreshResponse.json`
- `codex-rs/app-server-protocol/schema/json/v2/NullableMcpServerRefreshParams.json`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadPluginReloadParams.json`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadPluginReloadResponse.json`
- `codex-rs/app-server-protocol/schema/json/v2/TurnStartIfIdleParams.json`
- `codex-rs/app-server-protocol/schema/json/v2/TurnStartIfIdleResponse.json`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_custom.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_fast_mode_enabled.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_first_actions.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_global_chord.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_narrow.snap`
- `codex-rs/tui/src/snapshots/codex_tui__keymap_setup__tests__keymap_picker_wide.snap`

The tracked upstream workflow `codex-rs/vendor/bubblewrap/.github/workflows/check.yml` is intentionally excluded by the existing export filter: this import does not enable upstream automation. The exact exclusion and reason remain in `upstream/source-inventory.json`; `checks/.gitattributes` and operational checks remain excluded as well.

These generated resources remain source/build/test inputs under their existing notices. Executables, packages, operational checks, logs, credentials, profiles, receipts and private desktop assets are excluded. Existing synthetic AWS credential fixtures are public test data inherited from upstream, not credentials to any account; they remain unchanged.

## Historical r22 record

The following earlier record and the original `upstream/r22.patch` and build-input hash catalog are retained for project history. They describe r22, not the current snapshot.

# r22 local modifications

OpenDIY Codex is an independent project. The upstream Apache-2.0 license and copyright notices are retained; upstream LICENSE and NOTICE are also preserved under `upstream/`.

The official source baseline is `6905ecede7816e91373e9eabda3f6b0368fc747b`. Local r22 changes are frozen at `f74c7574ae39b95550d6b445f828ba1010787f74`. The exact original change is `upstream/r22.patch`; its SHA-256 and the 23 changed files are listed in `SOURCE_BASELINE.json`.

The r22 change set covers MCP read lifetime/cancellation handling, Windows stdio and child/Job lifetime handling, related regression tests, and dependency lock updates. This description identifies the source change set and does not claim that a public release or all runtime behavior has been validated.

On 2026-10-26, a prominent one-line OpenDIY Codex modification notice was added to each of these 23 files using the file format's comment syntax (`//` for Rust and `#` for Cargo TOML/lock files). Existing copyright and license text was retained. Apart from this notice prefix, published source bytes equal the frozen Git blob. No uncommitted r23 source changes were imported.

`sourceSha256` identifies the frozen Git blob bytes; `publishedSha256` includes the notice prefix. `builtSourceSha256` separately identifies the recorded r22 Windows build input, which uses CRLF in 19 of the files. These hashes intentionally describe different byte representations. The published tree is not presented as byte-identical to the old build input or as a reproduced binary.

Unmodified third-party source remains under its existing license. `upstream/NOTICE` includes the original Ratatui attribution. The vendored bubblewrap license and copyright notices, including its `COPYING` text, are preserved unchanged.

## R27 work branch — 2026-10-29

Source implementation: `10d9ef1e0d80f379f4c0fc1dc343924f609dae66`. Adds opt-in buffered diagnostics, thread-owned context limits and Korean UI. This work branch is under integration testing; it is not a verified release or installation. Generated protocol fixtures are regenerated from the changed Rust definitions. Existing upstream notices and licenses are retained.

Changed source and generated fixture files:

- `codex-rs/Cargo.lock`
- `codex-rs/Cargo.toml`
- `codex-rs/app-server-protocol/schema/json/ServerNotification.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`
- `codex-rs/app-server-protocol/schema/json/v2/ThreadSettingsUpdatedNotification.json`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/app-server-protocol/schema/typescript/v2/ThreadSettings.ts`
- `codex-rs/app-server-protocol/src/export.rs`
- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/tests.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`
- `codex-rs/app-server/src/lib.rs`
- `codex-rs/app-server/src/request_processors/persisted_resume_settings_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_processor_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_summary.rs`
- `codex-rs/app-server/src/request_processors/turn_processor.rs`
- `codex-rs/app-server/src/thread_state.rs`
- `codex-rs/app-server/tests/suite/v2/thread_settings_update.rs`
- `codex-rs/cli/src/config_override_args.rs`
- `codex-rs/cli/src/localization.rs`
- `codex-rs/cli/src/main.rs`
- `codex-rs/core/src/agent/control_tests.rs`
- `codex-rs/core/src/codex_thread.rs`
- `codex-rs/core/src/session/handlers.rs`
- `codex-rs/core/src/session/mod.rs`
- `codex-rs/core/src/session/session.rs`
- `codex-rs/core/src/session/step_activation.rs`
- `codex-rs/core/src/session/step_activation_tests.rs`
- `codex-rs/core/src/session/tests.rs`
- `codex-rs/core/src/session/thread_settings.rs`
- `codex-rs/core/src/session/turn_context.rs`
- `codex-rs/core/src/session/turn_input.rs`
- `codex-rs/core/tests/suite/settings_constraints.rs`
- `codex-rs/diagnostics/Cargo.toml`
- `codex-rs/diagnostics/src/batch_log.rs`
- `codex-rs/diagnostics/src/lib.rs`
- `codex-rs/exec/Cargo.toml`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/state/src/extract.rs`
- `codex-rs/thread-store/src/thread_metadata_sync.rs`
- `codex-rs/tui/Cargo.toml`
- `codex-rs/tui/src/app/app_server_event_targets.rs`
- `codex-rs/tui/src/app/event_dispatch.rs`
- `codex-rs/tui/src/app/tests.rs`
- `codex-rs/tui/src/app/thread_settings.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app_server_session.rs`
- `codex-rs/tui/src/bottom_pane/approval_overlay.rs`
- `codex-rs/tui/src/bottom_pane/experimental_features_view.rs`
- `codex-rs/tui/src/bottom_pane/footer.rs`
- `codex-rs/tui/src/bottom_pane/popup_consts.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/model_popups.rs`
- `codex-rs/tui/src/chatwidget/permissions_menu.rs`
- `codex-rs/tui/src/chatwidget/settings.rs`
- `codex-rs/tui/src/chatwidget/settings_popups.rs`
- `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- `codex-rs/tui/src/chatwidget/tests/app_server.rs`
- `codex-rs/tui/src/chatwidget/tests/slash_commands.rs`
- `codex-rs/tui/src/config_panel/config_panel_tests.rs`
- `codex-rs/tui/src/config_panel/mod.rs`
- `codex-rs/tui/src/config_panel/render.rs`
- `codex-rs/tui/src/i18n.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/onboarding/auth.rs`
- `codex-rs/tui/src/onboarding/auth/headless_chatgpt_login.rs`
- `codex-rs/tui/src/onboarding/trust_directory.rs`
- `codex-rs/tui/src/onboarding/welcome.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/startup_orchestration.rs`
- `codex-rs/tui/src/status_indicator_widget.rs`
- `docs/native-cli-r27.ko.md`

Source update `da5893f12d012899c29336db426868d94942ec3a` (integration in progress):

- `codex-rs/app-server/src/lib.rs`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/tui/src/startup_orchestration.rs`

Source update `012b105ff548caa7bbd64e67e6fb86d192320077` (integration in progress):

- `tools/windows-cli/README.md`
- `tools/windows-cli/package_cli.py`
- `tools/windows-cli/test_package_cli.py`

Source update `6b4628988df39ae5995f0f3706687d73e9d89ea9` (integration in progress):

- `codex-rs/Cargo.lock`
- `codex-rs/Cargo.toml`
- `codex-rs/app-server-daemon/Cargo.toml`
- `codex-rs/app-server-daemon/src/lib.rs`
- `codex-rs/app-server-daemon/src/remote_control_client.rs`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts`
- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/memory.rs`
- `codex-rs/app-server-transport/src/transport/remote_control/tests.rs`
- `codex-rs/app-server/src/main.rs`
- `codex-rs/app-server/src/main_tests.rs`
- `codex-rs/app-server/src/message_processor.rs`
- `codex-rs/app-server/src/request_processors/persisted_resume_settings_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_memory.rs`
- `codex-rs/app-server/tests/suite/v2/memory_read.rs`
- `codex-rs/app-server/tests/suite/v2/remote_control.rs`
- `codex-rs/cli/src/localization.rs`
- `codex-rs/cli/src/main.rs`
- `codex-rs/cli/src/remote_control_cmd.rs`
- `codex-rs/cloud-tasks/src/cli.rs`
- `codex-rs/cloud-tasks/src/env_detect.rs`
- `codex-rs/cloud-tasks/src/env_detect_tests.rs`
- `codex-rs/cloud-tasks/src/lib.rs`
- `codex-rs/cloud-tasks/src/util.rs`
- `codex-rs/config/src/lib.rs`
- `codex-rs/config/src/memory_scope.rs`
- `codex-rs/config/src/types.rs`
- `codex-rs/core/src/agent/control_tests.rs`
- `codex-rs/core/src/config/config_tests.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/session/mod.rs`
- `codex-rs/core/src/session/session.rs`
- `codex-rs/ext/memories/src/extension.rs`
- `codex-rs/ext/memories/src/prompts.rs`
- `codex-rs/ext/memories/src/prompts_tests.rs`
- `codex-rs/ext/memories/src/tests.rs`
- `codex-rs/ext/memories/templates/memories/read_path.md`
- `codex-rs/ext/memories/templates/memories/read_path_v2.md`
- `codex-rs/external-agent-migration/src/memory.rs`
- `codex-rs/external-agent-migration/src/memory_import.rs`
- `codex-rs/external-agent-migration/src/memory_import_tests.rs`
- `codex-rs/external-agent-migration/src/memory_tests.rs`
- `codex-rs/external-agent-migration/src/sessions/records_cla.rs`
- `codex-rs/memories/write/src/lib.rs`
- `codex-rs/memories/write/src/notes.rs`
- `codex-rs/memories/write/src/phase1.rs`
- `codex-rs/memories/write/src/phase2.rs`
- `codex-rs/memories/write/src/rollout_input.rs`
- `codex-rs/memories/write/src/runtime.rs`
- `codex-rs/memories/write/src/start.rs`
- `codex-rs/memories/write/src/startup_tests.rs`
- `codex-rs/protocol/src/memory_version.rs`
- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/state/src/extract.rs`
- `codex-rs/state/src/runtime/memories.rs`
- `codex-rs/state/src/runtime/memory_versions.rs`
- `codex-rs/state/src/runtime/memory_versions_tests.rs`
- `codex-rs/state/src/sqlite.rs`
- `codex-rs/state/src/sqlite_tests.rs`
- `codex-rs/thread-store/src/thread_metadata_sync.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/app/agent_navigation.rs`
- `codex-rs/tui/src/app/agent_picker.rs`
- `codex-rs/tui/src/app/config_persistence.rs`
- `codex-rs/tui/src/app/event_dispatch.rs`
- `codex-rs/tui/src/app/memory.rs`
- `codex-rs/tui/src/app/subagent_status_tests.rs`
- `codex-rs/tui/src/app/tests/local_command_scroll_tests.rs`
- `codex-rs/tui/src/app/thread_routing.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app_server_session.rs`
- `codex-rs/tui/src/bottom_pane/memories_settings_view.rs`
- `codex-rs/tui/src/bottom_pane/memories_settings_view_tests.rs`
- `codex-rs/tui/src/bottom_pane/mod.rs`
- `codex-rs/tui/src/bottom_pane/subagent_status.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- `codex-rs/tui/src/chatwidget/status_controls.rs`
- `codex-rs/tui/src/i18n.rs`
- `codex-rs/tui/src/slash_command.rs`

Source update `a7bd64a5888e7a6ccc58b175c4363584bd7ecc61` (integration in progress):

- `codex-rs/core/config.schema.json`
- `codex-rs/diagnostics/src/batch_log.rs`

Source update `5270c5df5701b71bc7ec9514a17838c8afcd22dc` (integration in progress):

- `codex-rs/cli/src/localization.rs`
- `codex-rs/tui/src/onboarding/trust_directory.rs`
- `codex-rs/tui/src/status_indicator_widget.rs`

소스 갱신 `b34fa5a1aab69b1e0044a23d34b153099b33813b` (통합 진행 중):

- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/tests/suite/v2/thread_read.rs`
- `codex-rs/core/src/session_rollout_init_error.rs`
- `codex-rs/core/src/session_rollout_init_error_tests.rs`
- `codex-rs/core/src/thread_manager/shutdown.rs`
- `codex-rs/thread-store/src/error.rs`
- `codex-rs/thread-store/src/lib.rs`
- `codex-rs/thread-store/src/local/thread_history/read.rs`
- `codex-rs/thread-store/src/local/thread_history/read_tests.rs`
- `codex-rs/thread-store/src/local/thread_history/segment_paging.rs`
- `codex-rs/thread-store/src/local/thread_history_materialization_tests.rs`

소스 갱신 `7af94cabdab420bc52f43c152b900bf345279aee` (통합 진행 중):

- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/src/request_processors/thread_processor_tests.rs`
- `codex-rs/thread-store/src/local/thread_history_materialization_tests.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/app/agent_message_consolidation.rs`
- `codex-rs/tui/src/app/event_dispatch.rs`
- `codex-rs/tui/src/app/history_pagination.rs`
- `codex-rs/tui/src/app/history_pagination_tests.rs`
- `codex-rs/tui/src/app/history_ui.rs`
- `codex-rs/tui/src/app/history_window.rs`
- `codex-rs/tui/src/app/owned_transcript.rs`
- `codex-rs/tui/src/app/session_lifecycle.rs`
- `codex-rs/tui/src/app/startup.rs`
- `codex-rs/tui/src/app/test_support.rs`
- `codex-rs/tui/src/app/tests.rs`
- `codex-rs/tui/src/app/tests/history_hydration_tests.rs`
- `codex-rs/tui/src/app/tests/pagination_completion_tests.rs`
- `codex-rs/tui/src/app/thread_event_buffer.rs`
- `codex-rs/tui/src/app/thread_event_buffer_tests.rs`
- `codex-rs/tui/src/app/thread_events.rs`
- `codex-rs/tui/src/app/thread_routing.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app_server_session.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/app_server_session/history_tests.rs`
- `codex-rs/tui/src/app_server_session/rollout_history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history_tests.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/activity_groups.rs`
- `codex-rs/tui/src/chatwidget/command_lifecycle.rs`
- `codex-rs/tui/src/chatwidget/dynamic_activity.rs`
- `codex-rs/tui/src/chatwidget/streaming.rs`
- `codex-rs/tui/src/chatwidget/tests/reasoning_status_tests.rs`
- `codex-rs/tui/src/chatwidget/tool_lifecycle.rs`
- `codex-rs/tui/src/chatwidget/transcript.rs`
- `codex-rs/tui/src/exec_cell/live_output.rs`
- `codex-rs/tui/src/exec_cell/model.rs`
- `codex-rs/tui/src/exec_cell/render.rs`
- `codex-rs/tui/src/history_cell/activity_details.rs`
- `codex-rs/tui/src/history_cell/base.rs`
- `codex-rs/tui/src/history_cell/computer_activity.rs`
- `codex-rs/tui/src/history_cell/computer_activity_tests.rs`
- `codex-rs/tui/src/history_cell/dynamic.rs`
- `codex-rs/tui/src/history_cell/mcp.rs`
- `codex-rs/tui/src/history_cell/mcp_result.rs`
- `codex-rs/tui/src/history_cell/messages.rs`
- `codex-rs/tui/src/history_cell/mod.rs`
- `codex-rs/tui/src/history_window.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/pager_overlay/transcript.rs`
- `codex-rs/tui/src/thread_transcript/computer_groups.rs`
- `codex-rs/tui/src/thread_transcript/exploration_groups.rs`
- `codex-rs/tui/src/transcript_view/layout.rs`
- `codex-rs/tui/src/transcript_view/snapshot.rs`

소스 갱신 `8dfe79b00aeb9f950a86f2baaad9ea81b45b8921` (통합 진행 중):

- `codex-rs/app-server/src/request_processors/thread_attachments.rs`
- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/src/request_processors/thread_processor_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_sections.rs`
- `codex-rs/app-server/tests/suite/v2/thread_read.rs`
- `codex-rs/core/src/session_rollout_init_error.rs`
- `codex-rs/core/src/session_rollout_init_error_tests.rs`
- `codex-rs/core/src/thread_manager/shutdown.rs`
- `codex-rs/thread-store/src/error.rs`
- `codex-rs/thread-store/src/lib.rs`
- `codex-rs/thread-store/src/local/thread_history/read.rs`
- `codex-rs/thread-store/src/local/thread_history/read_tests.rs`
- `codex-rs/thread-store/src/local/thread_history/segment_paging.rs`
- `codex-rs/tui/src/app/history_pagination.rs`
- `codex-rs/tui/src/app/history_window.rs`
- `codex-rs/tui/src/app/native_history.rs`
- `codex-rs/tui/src/app/native_history_tests.rs`
- `codex-rs/tui/src/app/tests/history_hydration_tests.rs`
- `codex-rs/tui/src/app/thread_event_buffer.rs`
- `codex-rs/tui/src/app/thread_event_buffer_tests.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/chatwidget/streaming.rs`
- `codex-rs/tui/src/chatwidget/tests/copy_export_picker_tests.rs`
- `codex-rs/tui/src/chatwidget/transcript.rs`
- `codex-rs/tui/src/history_window.rs`

소스 갱신 `d10723ca45f45db13539865ac13d6892660fb788` (통합 진행 중):

- `codex-rs/app-server-protocol/src/protocol/v2/mcp.rs`
- `codex-rs/core/src/session/step_activation_tests.rs`
- `codex-rs/ext/memories/src/prompts_tests.rs`
- `codex-rs/thread-store/src/local/thread_history_materialization_tests.rs`

소스 갱신 `11776c706192892fa932db3bca7cc2d8d5b4ecb6` (통합 진행 중):

- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`
- `codex-rs/app-server-protocol/schema/typescript/v2/McpServerRefreshResponse.ts`

소스 갱신 `68eb12b9f5eaaf084ec6cf9d97809394d27f714a` (통합 진행 중):

- `codex-rs/tui/src/app/agents_overview_tests.rs`
- `codex-rs/tui/src/app/history_pagination.rs`
- `codex-rs/tui/src/app/snapshots/codex_tui__app__daemon_menu__tests__daemon_cli_confirmation.snap`
- `codex-rs/tui/src/app/snapshots/codex_tui__app__owned_transcript__empty_state_animation_tests__fresh_thread_header.snap`
- `codex-rs/tui/src/app/snapshots/codex_tui__app__tests__app_server_thread_replacement_clears_previous_transcript_before_replay-2.snap`
- `codex-rs/tui/src/app/tests.rs`
- `codex-rs/tui/src/app/tests/history_hydration_tests.rs`
- `codex-rs/tui/src/app/tests/local_command_scroll_tests.rs`
- `codex-rs/tui/src/app/tests/safety_buffering.rs`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__active_reconnect__unavailable_conversation.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_buffered_replay_reconciles_captions.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_completed_after_thread_switch.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_partial_completed_after_thread_switch.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__realtime_requests__voice_replay_reconciles_matching_captions.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__session_lifecycle_requests__new_session_tests__widget_replacement_preserves_mention_usage_with_history_disabled.snap`
- `codex-rs/tui/src/app/tests/snapshots/codex_tui__app__tests__startup_frame_tests__startup_tip_in_transcript.snap`
- `codex-rs/tui/src/app/thread_events.rs`
- `codex-rs/tui/src/app/thread_routing.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history_tests.rs`
- `codex-rs/tui/src/chatwidget/slash_dispatch.rs`
- `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__memories_settings_popup.snap`
- `codex-rs/tui/src/chatwidget/tests/slash_commands.rs`
- `codex-rs/tui/src/history_cell/snapshots/codex_tui__history_cell__tests__pnpm_update_available_history_cell_snapshot.snap`
- `codex-rs/tui/src/history_cell/snapshots/codex_tui__history_cell__tests__standalone_unix_update_available_history_cell_snapshot.snap`
- `codex-rs/tui/src/history_cell/snapshots/codex_tui__history_cell__tests__standalone_windows_update_available_history_cell_snapshot.snap`
- `codex-rs/tui/src/pets/mod.rs`
- `codex-rs/tui/src/snapshots/codex_tui__app__agents_overview__tests__agents_overview_directory_grouping.snap`
- `codex-rs/tui/src/snapshots/codex_tui__app__tests__selected_unloaded_subagent_V1.snap`
- `codex-rs/tui/src/snapshots/codex_tui__startup_draft__layout__tests__owned_startup_layout.snap`
- `codex-rs/tui/src/snapshots/codex_tui__update_prompt__tests__update_picker_selected_28x12.snap`
- `codex-rs/tui/src/snapshots/codex_tui__update_prompt__tests__update_prompt_modal.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_server_auth_not_required.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_cached_limits_hide_credits_without_flag.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_credits_and_limits.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_enterprise_monthly_credit_limit.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_forked_from.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_monthly_limit.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_includes_reasoning_details.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_active_user_defined_profile.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_auto_review_permissions.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_chatgpt_plan_without_email.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_missing_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_refreshing_limits_notice.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_stale_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_shows_unavailable_limits_message.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_truncates_halfwidth_kana_in_narrow_terminal.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_truncates_in_narrow_terminal.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_uses_default_reasoning_when_config_empty.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_uses_generic_limit_labels_for_unsupported_windows.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__transcript_overlay_status_rate_limit_refresh.snap`

소스 갱신 `d686f8d2d2eec5d4f625a90709efec209a0adee7` (통합 진행 중):

- `codex-rs/app-server-daemon/src/remote_control_client.rs`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/tui/src/app_server_session/history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history.rs`
- `codex-rs/tui/src/app_server_session/rollout_history_tests.rs`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_server_auth_required.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_remote_server.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_wraps_enterprise_monthly_credit_details_in_narrow_terminal.snap`
- `docs/native-cli-r27.ko.md`

소스 갱신 `9766a8b7b8c4d56d7076367ddc12f2917a3df197` (통합 진행 중):

- `codex-rs/diagnostics/Cargo.toml`
- `codex-rs/diagnostics/src/batch_log.rs`
- `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs`
- `codex-rs/tui/src/snapshots/codex_tui__app__tests__selected_unloaded_subagent_V2.snap`
- `codex-rs/tui/src/status/snapshots/codex_tui__status__tests__status_snapshot_local_background_server.snap`
- `docs/native-cli-r27.ko.md`

## 공유 로컬 서버 재접속·기본 프록시 — 2026-10-30

이전 고정 소스 `9766a8b7b8c4d56d7076367ddc12f2917a3df197`는 기존 자연어 예약 연결에 다음 수정을 더했다. 고정 원본 소스의 표적 시험·전체 TUI 시험과 일반 개발 실행 파일의 제한된 격리 시험을 완료했다. 독립감사 07의 통과는 캐시 복구와 해당 개발 실행 파일의 격리 실행 범위에 한정되며, 전체 제품 완료·설치 가능 상태는 `BLOCKED`다. 실행 방법과 실제 수치는 시험 안내에서 구분한다. 기존 R28 실행 파일이나 앞선 후보의 결과를 이번 후보의 검증 근거로 사용하지 않는다.

- 실제로 로드된 미저장 로컬 대화는 `excludeTurns=true`이고 초기 이력 페이지를 요구하지 않을 때 현재 작성자 런타임과 세션 식별자·설정을 유지해 다른 연결에서 재개할 수 있다. 저장 이력이 필요한 요청, 로드되지 않은 대화와 임의 UUID는 이 예외에 포함하지 않는다. 저장소가 이력이 없다고 판정한 `None` 경로에서는 이미 저장된 작성자를 새 미저장 작성자용 대체 경로로 흡수하지 않는다. SQLite 메타데이터가 `Some`인 로드된 대화의 메타데이터 전용 재개는 파일 삭제 여부와 별개로 허용하던 기존 조회 계약을 유지한다.
- 소켓 경로를 생략한 기본 `app-server proxy`는 Windows에서 기존 비공개 연결 경로 검증과 비상승 권한 상대 검사를 수행한 뒤에만 데이터를 전달한다. 경로 검증 핸들은 접속·상대 인증이 끝날 때까지 유지한다. 명시적 소켓 경로와 Windows 이외 경로의 기존 동작은 유지한다.
- 작성한 회귀는 프록시 3개 함수의 3사례, 재개 4개 함수의 10사례로 총 13사례이며 표적 8개 그룹의 107개 통과 결과에 포함된다. 별도의 전체 TUI 시험은 5,849개 통과·0개 실패·4개 무시였다. 명령과 범위는 [시험 안내](docs/TESTING.md#공유-로컬-서버-후속-회귀시험검증-범위)에 적었다.

이전 후보 `eab9e295e54453bc30e00ae93c85d39ba738f596`의 독립감사 05 결과는 `FAIL`이었다. 당시 후보 `9766a8b7b8c4d56d7076367ddc12f2917a3df197`는 그 판정을 이어받지 않는다. 이전 갱신은 SQLite 설정별 회귀·MCP 시험 요청·판정 및 예약 도구 노출 기대값을 보강했고, 이번 갱신에는 미저장 대화의 커서 저장소 조건 수정도 포함된다.

MCP 회귀 요청에는 필수 `io.modelcontextprotocol/protocolVersion`·`io.modelcontextprotocol/clientCapabilities` 메타데이터를 넣어 전송 계층 거부를 예약 큐 검사로 오인하지 않도록 했다. 부정 응답은 정확한 오류 코드·메시지를 검사하고, 요청이 큐에 도착하기 전에 응답되면 본문을 표시하도록 시험을 보강했다. 해당 회귀를 포함한 실행 결과와 제한은 시험 안내에 기록했다.

이전 자연어 예약 후보 `921d404013da2035f90e9ab055acadc957a88592`의 전체 TUI 시험은 5,845개 통과·4개 실패·4개 건너뜀으로 종료 코드 101이었다. 실패 중 2개는 MCP 요청의 필수 메타데이터 누락으로 `a3edca405d1eddf138f549764459394603576804`에서 보강했다. 나머지 2개는 새 `cron` 노출을 빠뜨린 도구 목록·`deferLoading` 기대값으로 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609`에서 보강했다.

이번 시험 보강은 기존 작업트리 MCP에 `cron` 하나와 작업트리 도구 3개만 노출되는지, 원래 이름 공간과 입력 인자 제한을 유지하는지, 일반 작업 생성 호출은 정확한 오류로 거부되는지 검사한다. 외부 전달 방식은 `cron`만 `deferLoading=false`, 기존 비위임 도구 6개는 `true`로 남고 위임 도구는 포함하지 않도록 검사한다.

직전 `b4a80da10d5c7ec6e6b5f2de3dda1789b6e79609` 후보의 표적 실행은 100개 통과·7개 실패였고, 8개 실행 그룹 중 3개 통과·5개 실패였다. 이전 `9766a8b7b8c4d56d7076367ddc12f2917a3df197` 후보에서는 표적 107개 통과·0개 실패와 전체 TUI 5,849개 통과·0개 실패·4개 무시를 확인했다. 이전 후보별 실패와 통과 기록은 새 후보의 진행 상황과 구분한다.

이번 제품 변경은 저장 메타데이터가 없는 미저장 대화의 페이징 재개에서 커서 저장소를 붙이지 않는 조건으로 한정한다. 나머지는 실제 원격 대화의 기록 준비와 같은 `sessionId` 확인, `deferLoading=false` 필드 생략·역직렬화 기본값 검사, 새 `cron` 도구가 포함된 작업트리 목록 스냅샷, 임시 접근 제어 목록 시험의 `Set-Acl`을 `icacls`로 바꾸는 보강이다. 접근 제어 목록 시험은 감사 권한을 요청하지 않는다. 빈 미저장 대화의 실제 화면 전환까지 해결됐다고 주장하지 않는다.

일반 개발 실행 파일의 제한된 격리 시험에서는 기본 프록시의 두 이력 형식 동일 세션 재개·프록시 종료 후 서버 유지와 용량 오류 세 조건을 확인했다. 이전 transport 메타데이터 재사용으로 실패한 같은 소스의 빌드는 관련 지문 6개만 보존 이동한 뒤 성공했다. 최초 유입 경위와 향후 재혼입 방지는 미확인이다. [실제 시험 수치·캐시 복구·감사 07 범위](docs/TESTING.md#공유-로컬-서버-후속-회귀시험검증-범위)를 참고한다.

관련 소스·시험 경로:

- `codex-rs/cli/src/main.rs`
- `codex-rs/stdio-to-uds/Cargo.toml`
- `codex-rs/stdio-to-uds/src/lib.rs`
- `codex-rs/stdio-to-uds/src/local_daemon_tests.rs`
- `codex-rs/app-server/src/request_processors/thread_processor.rs`
- `codex-rs/app-server/tests/suite/v2/thread_resume.rs`
- `codex-rs/tui/src/app/cron_jobs.rs`
- `codex-rs/tui/src/app/tests/active_reconnect_tests.rs`

- `codex-rs/tui/src/app/tests/session_lifecycle_requests.rs`
- `codex-rs/tui/src/managed_worktree_tools_tests.rs`

- `codex-rs/uds/src/lib_tests.rs`
- `codex-rs/tui/src/snapshots/codex_tui__managed_worktree_tools__tests__model_tests__managed_worktree_model_creation.snap`

## 빈 미저장 대화 이력 복원 후속 후보 — 2026-10-30

이전 고정 소스 `67404f486047863ff7e56d7f47f4c350a872b483`는 `004c9113ff02c292998f1fa03355afe3dfe6d404` 이후 시험 3파일만 보강한 후보다. 제품 코드는 동일하다. 직전 `4f`의 화면·계약 시험은 통과했지만 전체 TUI 시험에서 1개가 실패했으며 시험 안내에 기록했다. 직전 `92d`의 06 TUI 컴파일은 시험 준비 호출의 `anyhow::Error`를 `eyre`로 자동 변환하지 못한 `E0277`로 종료 코드 101이었고 실행시험·일반 실행 파일 빌드는 미실행이다. `704`는 그 호출 한 곳을 인접 시험과 같은 명시적 준비 실패 `expect`로 바꿨다. `704`의 07 시험 컴파일은 통과했지만 대화 선택 시험은 1개 통과·2개 실패였고 일반 개발 빌드는 미실행이다. 이번 `674`는 설정 변경 알림과 `includeTurns=false` 생략 계약을 시험에 반영한 수정이다. 08 일반 개발 CLI·TUI 시험 컴파일과 영향 시험 66개, 고정 개발 실행 파일의 격리 시험 두 실행기는 종료 코드 0이었다. 전체 TUI를 새로 전량 통과한 결과는 아니다. 독립감사 08은 고정 소스·개발 실행 파일·66개 영향 시험·격리 실행에 한정해 PASS였다. 이후 최적화 CLI·별도 패키지 검사와 실제 Desktop 공유 실행도 확인했고 독립감사 09도 별도 패키지·최적화 격리 실행·공유 8검사 범위에서 `PASS(한정)`였다. 사용자 승인 후 새 명령줄 바로가기·기본 공유 서버를 전환했고 사후감사 11은 그 범위에서 `PASS(한정)`였다. 모바일·장시간 부하는 미완료다. 범위는 시험 안내에 구분했다. 이전 `9766` 후보의 표적 107개·전체 TUI 5,849개 통과와 고정 개발 실행 파일·감사 07 결과는 이번 후보의 통과 근거가 아니다.

서버는 같은 로컬 작성자가 실제로 로드돼 있고 영속 파일 생성을 미룬 상태인 초기 이력 조회의 `-32600` 오류에만 `kind=historyProjectionUnavailable`, `reason=unmaterialized` 표식을 더한다. 기존 코드·메시지는 유지하며 삭제된 저장 이력, 커서·고정점 요청과 다른 오류에 이 예외를 넓히지 않도록 작성했다.

TUI는 성공한 재개와 원래 세션·연결을 유지하면서 이력 미조회 상태를 보존하고 입력·실시간 이벤트를 계속 처리하도록 변경했다. 오류를 한 번 표시하거나 새 이벤트를 받았다는 이유로 이력을 조회 완료로 바꾸지 않는다. 빈 재생에도 종료 이벤트를 보내 현재 표시 대화의 이력 상태를 반영하고, 이전 커서가 없는 `Home` 동작이 로딩 상태에 고정되지 않도록 했다. 명시적인 이력 조회 성공 뒤에만 해당 미조회 상태를 해제하도록 작성했다.

실제 원격 요청·응답과 화면 이벤트를 통과하는 회귀, 오류 코드·표식의 부정 경계, 삭제된 저장 이력의 기존 계약 검사를 보강했다. 소스·시험 코드를 반영한 것이며 새 후보의 실행 결과나 전체 제품 완료를 뜻하지 않는다. [후속 시험 방법](docs/TESTING.md#빈-미저장-대화-이력-복원-후속-후보)을 참고한다.

## R33 Windows updater follow-up (2026-11-03)

- Development source: `bd2ed8ec7854d64905caa67d0f77758e758a930c`; this is a source-only repair candidate, not a rebuilt or reinstalled release.
- Resolve task-owner account spellings to SIDs while preserving exact action and Limited-privilege checks.
- Emit UTF-8 from the updater process so Korean installation paths survive native CLI forwarding and hidden execution.
- Add native-host output regression and extend existing scheduler ownership tests. The published R33 assets remain unchanged.

## R38 개발 후보 — 2026-11-06

고정 소스 `b8989c4c3d2537800e7d5fdb8990a5572f8205f2`는 R37 이후의 별도 개발 후보다. 플러그인 payload 비교에서 정확한 `.git`만 제외하고, 암시적 daemon의 전체 버전 확인과 실제 시작·재연결 화면 안내를 보강했다. Stop 속성의 명시적 교정 도구와 재로드 경고의 한국어/영어 안내, 에이전트 목록의 메인 마우스 복귀 및 작은 밑줄만 사용하는 hover를 포함한다.

요구별 실행 결과·원래 실패·미검증 경계는 [R38 범위](docs/native-cli-r38.ko.md)와 연결된 사례 문서를 따른다. 운영 설정 자동 교정·R38 최종 릴리스 빌드·공개·설치는 수행하지 않았다. R37 동결 실행 파일은 변경하지 않았다.

## R38 개발 후보 — 2026-11-06

고정 소스 `8e9b7eff953e750e8f071d101233920108e5ca26`는 R37 이후의 별도 개발 후보다. 플러그인 payload 비교에서 정확한 `.git`만 제외하고, 암시적 daemon의 전체 버전 확인과 실제 시작·재연결 화면 안내를 보강했다. Stop 속성의 명시적 교정 도구와 재로드 경고의 한국어/영어 안내, 에이전트 목록의 메인 마우스 복귀 및 작은 밑줄만 사용하는 hover를 포함한다.

요구별 실행 결과·원래 실패·미검증 경계는 [R38 범위](docs/native-cli-r38.ko.md)와 연결된 사례 문서를 따른다. 운영 설정 자동 교정·R38 최종 릴리스 빌드·공개·설치는 수행하지 않았다. R37 동결 실행 파일은 변경하지 않았다.
