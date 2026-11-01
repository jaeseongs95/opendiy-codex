// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! UI-only language selection. Never apply this dictionary to prompts, tool arguments,
//! slash command tokens, paths, or text supplied by the user or a server.
//!
//! `CODEX_UI_LANGUAGE=ko|en` overrides the saved base user `ui_language`; default is English.
//! Before startup initialization, library callers retain the original English strings.

use std::io;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Korean,
    English,
}

impl Locale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Korean => "ko",
            Self::English => "en",
        }
    }

    pub fn from_setting(setting: Option<&str>) -> Result<Self, &'static str> {
        match setting {
            Some("ko") => Ok(Self::Korean),
            None | Some("en") => Ok(Self::English),
            Some(_) => Err("CODEX_UI_LANGUAGE must be ko or en (한국어: ko, English: en)"),
        }
    }
}

static LOCALE: OnceLock<Locale> = OnceLock::new();

/// Call before constructing CLI help or TUI views. This never changes configuration files.
pub fn initialize() -> io::Result<()> {
    // CLI and TUI startup can both call this in one process. A later global file
    // edit is for the next process and must not invalidate this launch's locale.
    if LOCALE.get().is_some() {
        return Ok(());
    }
    let setting = match std::env::var("CODEX_UI_LANGUAGE") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "CODEX_UI_LANGUAGE must contain Unicode ko or en",
            ));
        }
    };
    let home = codex_utils_home_dir::find_codex_home()?;
    let locale = locale_from_user_settings(home.as_path(), setting.as_deref())?;
    // A concurrent initializer keeps the first successful selection for this process.
    let _ = LOCALE.set(locale);
    Ok(())
}

fn locale_from_user_settings(
    home: &Path,
    environment_override: Option<&str>,
) -> io::Result<Locale> {
    if environment_override.is_some() {
        return Locale::from_setting(environment_override)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error));
    }
    Ok(
        match codex_config::ui_language::read_user_ui_language(home)? {
            codex_config::ui_language::UiLanguage::Korean => Locale::Korean,
            codex_config::ui_language::UiLanguage::English => Locale::English,
        },
    )
}

pub fn locale() -> Locale {
    #[cfg(test)]
    if let Some(locale) = TEST_LOCALE.with(std::cell::Cell::get) {
        return locale;
    }
    LOCALE.get().copied().unwrap_or(Locale::English)
}

/// Translate an explicitly selected UI literal, falling back to the supplied original.
pub fn text(original: &str) -> &str {
    text_for(locale(), original)
}

pub fn text_for(locale: Locale, original: &str) -> &str {
    if locale == Locale::English {
        return original;
    }
    match original {
        "Current UI language:" => "현재 표시 언어:",
        "Project, profile and remote settings do not control UI language." => {
            "프로젝트·프로필·원격 설정은 표시 언어에 적용되지 않습니다."
        }
        "save the global UI language for the next launch: /language ko|en" => {
            "전역 표시 언어 저장: /language ko|en (다음 실행부터 적용)"
        }
        "Usage: /language ko|en (saved globally; applies on the next launch; CODEX_UI_LANGUAGE takes precedence)" => {
            "사용법: /language ko|en (전역 저장·다음 실행부터 적용·CODEX_UI_LANGUAGE 우선)"
        }
        "UI language saved globally. It applies on the next launch. CODEX_UI_LANGUAGE takes precedence." => {
            "표시 언어를 전역으로 저장했습니다. 다음 실행부터 적용하며 CODEX_UI_LANGUAGE가 우선합니다."
        }
        "Failed to save UI language:" => "표시 언어를 저장하지 못했습니다:",
        "Press " => "키: ",
        " to confirm or " => " 확인 / ",
        " to go back" => " 돌아가기",
        "select" => "선택",
        "back" => "뒤로",
        "to confirm" => "확인",
        "to cancel" => "취소",
        " or " => " 또는 ",
        " to open thread" => " 대화 열기",
        "Thread: " => "대화: ",
        "Environment: " => "환경: ",
        "Reason: " => "이유: ",
        "Permission rule: " => "권한 규칙: ",
        "Input: " => "입력: ",
        "Server: " => "서버: ",
        "Would you like to send input to the existing terminal?" => {
            "기존 터미널에 입력을 보내시겠습니까?"
        }
        "Would you like to run the following command?" => "다음 명령을 실행하시겠습니까?",
        "Would you like to grant these permissions?" => "다음 권한을 허용하시겠습니까?",
        "Would you like to make the following edits?" => "다음 파일 변경을 적용하시겠습니까?",
        "You did not grant additional permissions" => "추가 권한을 허용하지 않았습니다",
        "You granted additional permissions with strict auto review" => {
            "엄격한 자동 검토를 적용하여 추가 권한을 허용했습니다"
        }
        "You granted additional permissions for this session" => {
            "이 세션에 추가 권한을 허용했습니다"
        }
        "You granted additional permissions" => "추가 권한을 허용했습니다",
        "Yes, just this once" => "예, 이번 한 번만 허용",
        "Yes, proceed" => "예, 진행",
        "Yes, and allow this host for this conversation" => "예, 이 대화에서 이 호스트 허용",
        "Yes, and allow these permissions for this session" => "예, 이 세션에 해당 권한 허용",
        "Yes, and don't ask again for this command in this session" => {
            "예, 이 세션에서 이 명령을 다시 묻지 않기"
        }
        "Yes, and allow this host in the future" => "예, 앞으로 이 호스트 허용",
        "No, and block this host in the future" => "아니요, 앞으로 이 호스트 차단",
        "No, continue without running it" => "아니요, 실행하지 않고 계속",
        "No, and tell Codex what to do differently" => "아니요, Codex에 수정할 내용을 지시",
        "Yes, and don't ask again for these files" => "예, 이 파일들에 대해 다시 묻지 않기",
        "Yes, grant these permissions for this turn" => "예, 이번 응답에 해당 권한 허용",
        "Yes, grant for this turn with strict auto review" => {
            "예, 엄격한 자동 검토로 이번 응답에 허용"
        }
        "Yes, grant these permissions for this session" => "예, 이 세션에 해당 권한 허용",
        "No, continue without permissions" => "아니요, 권한 없이 계속",
        "Yes, provide the requested info" => "예, 요청한 정보 제공",
        "No, but continue without it" => "아니요, 정보 없이 계속",
        "Cancel this request" => "이 요청 취소",
        "Waiting for history to be saved" => "이력 저장 대기 중",
        "Waiting for history to be saved.  " => "이력 저장 대기 중.  ",
        "Waiting for save" => "저장 대기 중",
        "Working" => "작업 중",
        " to interrupt)" => " 중단)",
        " for agents" => " 에이전트",
        " for shortcuts" => " 단축키",
        " to queue message" => " 메시지 대기열에 추가",
        " to queue" => " 대기열에 추가",
        " again to quit" => " 한 번 더 눌러 종료",
        " again to edit previous message" => " 한 번 더 눌러 이전 메시지 편집",
        " to edit previous message" => " 이전 메시지 편집",
        "Pursuing goal" => "목표 수행 중",
        "Goal paused (/goal resume)" => "목표 일시 중지 (/goal resume)",
        "Goal stalled (/goal resume)" => "목표 진행이 막힘 (/goal resume)",
        "Goal hit usage limits (/goal resume)" => "사용 한도로 목표 중지 (/goal resume)",
        "Goal abandoned" => "목표 미달성으로 종료",
        "Goal achieved" => "목표 달성",
        "IDE context" => "IDE 문맥",
        "Update Model Permissions" => "모델 권한 변경",
        "Current permission profile" => "현재 권한 프로필",
        "Not available on this server" => "이 서버에서 사용할 수 없음",
        "No permission profiles returned by the server." => {
            "서버가 권한 프로필을 반환하지 않았습니다."
        }
        "Read workspace files, with approval required for edits or internet access" => {
            "작업 공간 파일을 읽습니다. 수정과 인터넷 접근에는 승인이 필요합니다"
        }
        "Read and edit workspace files and run commands, with approval required for internet access or edits outside the workspace" => {
            "작업 공간 파일을 읽고 수정하며 명령을 실행합니다. 인터넷 접근과 작업 공간 밖 수정에는 승인이 필요합니다"
        }
        "Use with caution: Codex can edit files outside this workspace and access the internet without approval" => {
            "주의: 승인 없이 작업 공간 밖 파일을 수정하고 인터넷에 접근할 수 있습니다"
        }
        "Experimental features are unavailable until startup completes." => {
            "시작이 완료된 후 실험 기능을 사용할 수 있습니다."
        }
        "Approve for me is unavailable. Run `/permissions` to see available profiles." => {
            "자동 승인을 사용할 수 없습니다. `/permissions`에서 사용 가능한 프로필을 확인하세요."
        }
        "Internal error: missing the 'read-only' approval preset." => {
            "내부 오류: 'read-only' 승인 사전 설정이 없습니다."
        }
        "Internal error: missing the 'auto' approval preset." => {
            "내부 오류: 'auto' 승인 사전 설정이 없습니다."
        }
        "Internal error: missing the 'full-access' approval preset." => {
            "내부 오류: 'full-access' 승인 사전 설정이 없습니다."
        }
        "configure Codex preferences" => "Codex 환경 설정",
        "send logs to maintainers" => "유지보수 담당자에게 로그 보내기",
        "start a new chat during a conversation" => "새 대화 시작",
        "create an AGENTS.md file with instructions for Codex" => {
            "Codex 작업 지침을 담은 AGENTS.md 만들기"
        }
        "summarize conversation to prevent hitting the context limit" => {
            "문맥 한도에 도달하지 않도록 대화 압축"
        }
        "summarize the current conversation now" => "현재 대화를 지금 요약",
        "review my current changes and find issues" => "현재 변경을 검토하고 문제 찾기",
        "rename the current thread" => "현재 대화 이름 변경",
        "resume a saved chat" => "저장된 대화 이어가기",
        "archive this session" => "이 세션 보관",
        "permanently delete this session" => "이 세션 영구 삭제",
        "clear the terminal and start a new chat" => "터미널을 지우고 새 대화 시작",
        "fork the current chat" => "현재 대화 분기",
        "start or continue a conversation in a new worktree" => {
            "새 worktree에서 대화 시작 또는 이어가기"
        }
        "continue this session in the Desktop app" => "데스크톱 앱에서 이 세션 이어가기",
        "exit Codex" => "Codex 종료",
        "copy the last response or part of it" => "최근 응답 전체 또는 일부 복사",
        "export the conversation as markdown" => "대화를 Markdown으로 내보내기",
        "toggle raw scrollback mode for copy-friendly terminal selection" => {
            "복사하기 쉬운 원문 스크롤 모드 전환"
        }
        "choose the TUI mode for the next launch" => "다음 실행에 사용할 TUI 모드 선택",
        "show git diff (including untracked files)" => {
            "추적하지 않는 파일을 포함하여 git diff 표시"
        }
        "mention a file" => "파일 참조",
        "use skills to improve how Codex performs specific tasks" => "작업에 스킬 사용",
        "import setup, this project, and recent chats from Claude Code" => {
            "Claude Code 설정, 프로젝트, 최근 대화 가져오기"
        }
        "view and manage lifecycle hooks" => "수명 주기 훅 확인 및 관리",
        "Manage the local background server" => "로컬 백그라운드 서버 관리",
        "view retained warnings and diagnostic details" => "보관된 경고 및 진단 정보 확인",
        "show current session configuration and token usage" => "현재 세션 설정과 토큰 사용량 표시",
        "change the current working directory" => "현재 작업 디렉터리 변경",
        "show the current working directory" => "현재 작업 디렉터리 표시",
        "view account usage or use a usage limit reset" => "계정 사용량 확인 또는 사용 한도 초기화",
        "show config layers and requirement sources for debugging" => {
            "디버깅용 설정 계층과 요구사항 출처 표시"
        }
        "configure which items appear in the terminal title" => "터미널 제목에 표시할 항목 설정",
        "configure which items appear in the status line" => "상태 표시줄 항목 설정",
        "choose a syntax highlighting theme" => "구문 강조 테마 선택",
        "choose or hide the terminal pet" => "터미널 펫 선택 또는 숨기기",
        "list background terminals" => "백그라운드 터미널 목록",
        "turn Daybreak on or off" => "Daybreak 켜기 또는 끄기",
        "yield the current terminal while preserving its process" => {
            "프로세스를 유지하며 현재 터미널 양도"
        }
        "stop all background terminals" => "모든 백그라운드 터미널 중지",
        "choose what model and reasoning effort to use" => "모델과 추론 수준 선택",
        "run or save reusable prompts and slash commands" => {
            "재사용 프롬프트와 슬래시 명령 실행 또는 저장"
        }
        "include current selection, open files, and other context from your IDE" => {
            "IDE의 현재 선택, 열린 파일 및 문맥 포함"
        }
        "switch to Plan mode" => "계획 모드로 전환",
        "start or stop voice; use /voice settings to choose a voice" => {
            "음성 시작 또는 중지; /voice settings에서 음성 선택"
        }
        "set or view the goal for a long-running task" => "장시간 작업 목표 설정 또는 확인",
        "open the agent command center" => "에이전트 관리 화면 열기",
        "switch between this session's subagents" => "이 세션의 하위 에이전트 전환",
        "start a side conversation in an ephemeral fork" => "임시 분기에서 별도 대화 시작",
        "choose what Codex is allowed to do" => "Codex에 허용할 작업 선택",
        "remap TUI shortcuts" => "TUI 단축키 변경",
        "toggle Vim mode for the composer" => "입력창 Vim 모드 전환",
        "set up elevated agent sandbox" => "권한을 높인 에이전트 샌드박스 설정",
        "toggle experimental features" => "실험 기능 켜기 또는 끄기",
        "approve one retry of a recent auto-review denial" => {
            "최근 자동 검토 거부에 대해 한 번 재시도 승인"
        }
        "configure memory use and generation" => "메모리 사용과 생성 설정",
        "list MCP tools; use /mcp verbose or /mcp login <name>" => {
            "MCP 도구 목록; /mcp verbose 또는 /mcp login <name> 사용"
        }
        "manage apps" => "앱 관리",
        "browse plugins" => "플러그인 탐색",
        "reload plugins for this conversation without restarting" => {
            "재시작 없이 이 대화의 플러그인 다시 읽기"
        }
        "log out of Codex" => "Codex 로그아웃",
        "print the rollout file path" => "rollout 파일 경로 표시",
        "test approval request" => "승인 요청 시험",
        "Run Codex non-interactively" => "비대화형으로 Codex 실행",
        "Run a code review non-interactively" => "비대화형으로 코드 검토 실행",
        "Manage login" => "로그인 관리",
        "Remove stored authentication credentials" => "저장된 인증 정보 제거",
        "Manage external MCP servers for Codex" => "Codex 외부 MCP 서버 관리",
        "Manage Codex plugins" => "Codex 플러그인 관리",
        "Generate shell completion scripts" => "셸 자동 완성 스크립트 생성",
        "Update Codex to the latest version" => "Codex를 최신 버전으로 업데이트",
        "Diagnose local Codex installation, config, auth, and runtime health" => {
            "로컬 Codex 설치, 설정, 인증 및 실행 상태 진단"
        }
        "Run commands within a Codex-provided sandbox" => "Codex 샌드박스에서 명령 실행",
        "Debugging tools" => "디버깅 도구",
        "Inspect feature flags" => "기능 플래그 확인",
        "Optional user prompt to start the session" => "세션을 시작할 사용자 프롬프트",
        "Disable alternate screen mode" => "대체 화면 모드 사용 안 함",
        "Run without the shared background server, even if it is already running" => {
            "공유 백그라운드 서버가 실행 중이어도 연결하지 않고 실행"
        }
        "Print help" => "도움말 표시",
        "Print version" => "버전 표시",
        "show or set this chat's context window: /context [tokens|reset]" => {
            "이 대화의 문맥 창 확인 또는 설정: /context [tokens|reset]"
        }
        "Model selection is disabled until startup completes." => {
            "시작이 완료된 후 모델을 선택할 수 있습니다."
        }
        "Models are being updated; please try /model again in a moment." => {
            "모델 목록을 갱신 중입니다. 잠시 후 /model을 다시 실행하세요."
        }
        "Select Model" => "모델 선택",
        "Pick a quick auto mode or browse all models." => {
            "빠른 자동 모드를 선택하거나 모든 모델을 확인하세요."
        }
        "Select Model and Effort" => "모델과 추론 수준 선택",
        "No additional models are available right now." => {
            "현재 추가로 선택할 수 있는 모델이 없습니다."
        }
        "More reasoning…" => "추론 수준 더 보기…",
        "Advanced Reasoning" => "고급 추론",
        "⚠ Consumes usage limits faster" => "⚠ 사용 한도를 더 빨리 소모합니다",
        "Loading server experiments…" => "서버 실험 기능 불러오는 중…",
        "Experimental features" => "실험 기능",
        "Checked features are configured on. Some experimental features take effect only in new tasks or after restarting the Codex server." => {
            "선택한 기능이 켜집니다. 일부 실험 기능은 새 작업이나 Codex 서버 재시작 후 적용됩니다."
        }
        "Saving… Closing this popup will not cancel the write." => {
            "저장 중… 이 창을 닫아도 저장은 취소되지 않습니다."
        }
        "Selections retained. Save to retry, or cancel to close." => {
            "선택을 유지했습니다. 저장을 다시 시도하거나 취소하여 닫으세요."
        }
        " (read-only)" => " (읽기 전용)",
        "Saving experimental features…" => "실험 기능 저장 중…",
        "Saving was interrupted. Reopen /experimental to check configured values." => {
            "저장이 중단되었습니다. /experimental에서 설정값을 확인하세요."
        }
        "No server experiments available." => "사용 가능한 서버 실험 기능이 없습니다.",
        "Server experiments unavailable. Reopen /experimental to retry; restart this Codex client if requests remain unanswered." => {
            "서버 실험 기능을 사용할 수 없습니다. /experimental을 다시 여세요. 응답이 계속 없으면 Codex 클라이언트를 재시작하세요."
        }
        "  No experimental features available for now" => "  현재 사용 가능한 실험 기능이 없습니다",
        " toggle" => " 전환",
        " save" => " 저장",
        " save/close" => " 저장/닫기",
        "Plan mode" => "계획 모드",
        "Goal unmet" => "목표 미달성",
        "context left" => "문맥 남음",
        "used" => "사용",
        " to cycle)" => " 전환)",
        "  Choose how you want to use Codex." => "  Codex 사용 방법을 선택하세요.",
        "Sign in with ChatGPT to use Codex as part of your paid plan" => {
            "ChatGPT에 로그인하여 유료 플랜으로 Codex 사용"
        }
        "or connect an API key for usage-based billing" => {
            "또는 API 키를 연결하여 사용량에 따라 결제"
        }
        "ChatGPT login is disabled" => "ChatGPT 로그인이 비활성화되어 있습니다",
        "Usage included with Plus, Pro, Business, and Enterprise plans" => {
            "Plus, Pro, Business, Enterprise 플랜에 사용량 포함"
        }
        "Sign in from another device with a one-time code" => "일회용 코드로 다른 기기에서 로그인",
        "Sign in with ChatGPT" => "ChatGPT로 로그인",
        "Sign in with Device Code" => "기기 코드로 로그인",
        "Use an OpenAI API key" => "OpenAI API 키 사용",
        "Provide your own API key" => "내 API 키 입력",
        "Pay for what you use" => "사용한 만큼 결제",
        "Use Amazon Bedrock" => "Amazon Bedrock 사용",
        "Connect using your AWS credentials" => "AWS 인증 정보로 연결",
        "  API key login is disabled by this workspace. Sign in with ChatGPT to continue." => {
            "  이 작업 공간에서는 API 키 로그인을 사용할 수 없습니다. ChatGPT로 로그인하세요."
        }
        "  Press " => "  키: ",
        " to continue" => " 계속",
        "Finish signing in via your browser" => "브라우저에서 로그인을 완료하세요",
        "  If the link doesn't open automatically, press " => "  링크가 자동으로 열리지 않으면 ",
        " to copy it:" => " 키로 복사하세요:",
        "  On a remote or headless machine? Press " => "  원격 또는 화면 없는 기기에서는 ",
        " and choose " => " 키를 누르고 다음을 선택하세요: ",
        " to cancel" => " 취소",
        "  Learn about permissions and approvals in the " => "  권한과 승인에 관한 안내: ",
        "Codex docs" => "Codex 문서",
        "  Your plan's rate limits and " => "  플랜의 사용 한도 및 ",
        "training data preferences" => "학습 데이터 설정",
        " apply" => " 적용",
        "✓ Signed in with your ChatGPT account" => "✓ ChatGPT 계정으로 로그인했습니다",
        "  You're in control" => "  사용자가 작업을 제어합니다",
        "  Codex can make mistakes" => "  Codex는 실수할 수 있습니다",
        "  Review the code it writes and commands it runs" => {
            "  작성된 코드와 실행할 명령을 검토하세요"
        }
        "  Included with your ChatGPT plan" => "  ChatGPT 플랜에 포함됨",
        "✓ API key configured" => "✓ API 키를 설정했습니다",
        "  Codex will use usage-based billing with your API key." => {
            "  API 키 사용량에 따라 요금이 부과됩니다."
        }
        "Use your own OpenAI API key for usage-based billing" => {
            "내 OpenAI API 키를 사용하여 사용량에 따라 결제"
        }
        "  Paste or type your API key below." => "  아래에 API 키를 붙여 넣거나 입력하세요.",
        "  Detected OPENAI_API_KEY environment variable." => {
            "  OPENAI_API_KEY 환경 변수를 찾았습니다."
        }
        "  Paste a different key if you prefer to use another account." => {
            "  다른 계정을 사용하려면 다른 키를 붙여 넣으세요."
        }
        "Paste or type your API key" => "API 키를 붙여 넣거나 입력하세요",
        "API key" => "API 키",
        " to save" => " 저장",
        "API key cannot be empty" => "API 키를 입력하세요",
        "✓ Amazon Bedrock configured" => "✓ Amazon Bedrock을 설정했습니다",
        "Preparing device code login" => "기기 코드 로그인 준비 중",
        "  1. Open this link in your browser and sign in" => {
            "  1. 브라우저에서 이 링크를 열고 로그인하세요"
        }
        "  2. Enter this one-time code after you are signed in (expires in 15 minutes)" => {
            "  2. 로그인한 다음 이 일회용 코드를 입력하세요 (15분 후 만료)"
        }
        "  Continue only if you started this login in Codex. If a website or another person gave you this code, cancel." => {
            "  Codex에서 직접 로그인을 시작한 경우에만 진행하세요. 웹사이트나 다른 사람에게 받은 코드라면 취소하세요."
        }
        "  Requesting a one-time code..." => "  일회용 코드 요청 중...",
        "Welcome to " => "환영합니다: ",
        ", OpenAI's command-line coding agent" => ", OpenAI의 명령줄 코딩 에이전트",
        "Open existing task" => "기존 작업 열기",
        "Open restricted" => "제한 모드로 열기",
        "Trust and continue" => "신뢰하고 계속",
        "Quit" => "종료",
        "Back to Agent Command Center" => "에이전트 관리 화면으로 돌아가기",
        "Keep current directory" => "현재 디렉터리 유지",
        " continue and create sandbox" => " 계속하여 샌드박스 만들기",
        " continue" => " 계속",
        " quit" => " 종료",
        " back" => " 뒤로",
        " cancel" => " 취소",
        "Note: You’re in a subdirectory of a Git project. Trusting will apply to the repository root:" => {
            "참고: Git 프로젝트의 하위 디렉터리입니다. 신뢰 설정은 다음 저장소 루트에 적용됩니다:"
        }
        "  Folder access" => "  폴더 접근",
        "Automatic" => "자동",
        "Comfortable" => "넓게",
        "Dense" => "촘촘하게",
        "Ask" => "매번 확인",
        "Current directory" => "현재 디렉터리",
        "Session directory" => "세션 디렉터리",
        "Auto" => "자동",
        "On" => "켜짐",
        "Off" => "꺼짐",
        "Unfocused" => "다른 앱을 사용할 때",
        "Always" => "항상",
        "Configure" => "설정",
        "Appearance" => "화면",
        "Notifications" => "알림",
        "Input" => "입력",
        "Sessions" => "세션",
        "Text selection" => "텍스트 선택",
        "[x] On" => "[x] 켜짐",
        "[ ] Off" => "[ ] 꺼짐",
        "Send in 5s" => "5초 후 보내기",
        "Unavailable" => "사용할 수 없음",
        "Theme" => "테마",
        "Choose a syntax color palette with a live preview" => "미리보기로 구문 강조 색상 선택",
        "Effects" => "효과",
        "Configure animations and composer effects" => "애니메이션과 입력창 효과 설정",
        "Tips" => "도움말",
        "Show helpful shortcuts and suggestions" => "단축키와 제안 표시",
        "Status line colors" => "상태 표시줄 색상",
        "Use theme colors for the model, branch, and other status line details" => {
            "모델, 브랜치 등 상태 표시줄에 테마 색상 적용"
        }
        "Status line display" => "상태 표시줄 항목",
        "Choose and reorder the information below the composer" => {
            "입력창 아래 표시할 정보와 순서 선택"
        }
        "Terminal title display" => "터미널 제목 항목",
        "Choose and reorder the information in your terminal tab or window title" => {
            "터미널 탭이나 창 제목에 표시할 정보와 순서 선택"
        }
        "Let your terminal notify you when a turn finishes or approval is needed" => {
            "응답 완료나 승인 요청을 터미널 알림으로 받기"
        }
        "Event-specific notifications are configured in config.toml" => {
            "이벤트별 알림은 config.toml에서 설정합니다"
        }
        "Notify when" => "알림 시점",
        "Notify only while you are away or while this terminal is focused too" => {
            "다른 앱 사용 중에만 알리거나 이 터미널 사용 중에도 알리기"
        }
        "Turn on notifications to choose when they appear" => {
            "알림을 켜면 알림 시점을 선택할 수 있습니다"
        }
        "Test notification" => "알림 시험",
        "Send in 5 seconds so you can switch apps before the notification appears" => {
            "다른 앱으로 전환할 수 있도록 5초 후 알림 보내기"
        }
        "Keyboard shortcuts" => "키보드 단축키",
        "Browse actions and remap their keys" => "동작을 확인하고 단축키 변경",
        "Start in Vim mode" => "Vim 모드로 시작",
        "Start new composers in Vim mode and use /vim for the current composer" => {
            "새 입력창은 Vim 모드로 시작하며 현재 입력창은 /vim으로 전환"
        }
        "Paste detection" => "붙여넣기 감지",
        "Recognize pasted-text bursts and keep multiline input together" => {
            "붙여넣기를 감지하여 여러 줄 입력을 하나로 유지"
        }
        "Automatic recaps" => "자동 요약",
        "Prepare recaps while the terminal is unfocused" => "터미널을 사용하지 않는 동안 요약 준비",
        "Session picker layout" => "세션 목록 배치",
        "Choose tighter rows or a roomier list when resuming and forking conversations" => {
            "대화를 이어가거나 분기할 때 사용할 목록 간격 선택"
        }
        "Resume directory" => "이어갈 디렉터리",
        "Choose which working directory to use when the current directory and saved session differ" => {
            "현재 디렉터리와 저장된 세션의 디렉터리가 다를 때 사용할 위치 선택"
        }
        "Copy on select" => "선택 시 복사",
        "Copy transcript, footer, and composer selections on release; Auto follows terminal defaults" => {
            "마우스를 놓으면 선택한 대화, 하단 안내, 입력창 내용을 복사하며 자동은 터미널 기본값 적용"
        }
        "Deselect after copy" => "복사 후 선택 해제",
        "Remove read-only transcript and footer highlights after automatic copy succeeds" => {
            "자동 복사 성공 후 대화와 하단 안내의 선택 강조 해제"
        }
        "Turn on Copy on select to deselect after copying" => {
            "복사 후 선택을 해제하려면 선택 시 복사를 켜세요"
        }
        "Right-click paste" => "오른쪽 클릭으로 붙여넣기",
        "Animations" => "애니메이션",
        "Animate the welcome screen, working indicator, and transitions" => {
            "시작 화면, 작업 표시 및 전환에 애니메이션 사용"
        }
        "Composer stars" => "입력창 별 효과",
        "Show stars briefly for new Astra tasks" => "새 Astra 작업에서 별 효과를 잠시 표시",
        "Configure visual effects" => "시각 효과 설정",
        "Turn on Animations to show Astra task stars" => {
            "Astra 작업의 별 효과를 표시하려면 애니메이션을 켜세요"
        }
        "Auto (default)" => "자동 (기본값)",
        "Choose the working directory when resuming, with --cd taking precedence for this session" => {
            "대화를 이어갈 디렉터리 선택. 이번 세션에서는 --cd가 우선합니다"
        }
        "Requires --cd for remote sessions" => "원격 세션에는 --cd가 필요합니다",
        "Changes save automatically" => "변경 사항은 자동으로 저장됩니다",
        "Configuration" => "설정",
        "toggle" => "전환",
        "group" => "그룹",
        "search" => "검색",
        "clear search/close" => "검색 지우기/닫기",
        "open" => "열기",
        "test" => "시험",
        "clear search" => "검색 지우기",
        "Type to search settings" => "설정 검색",
        "This existing task may retain settings and history, including project configuration or hooks loaded while it was trusted. To use restricted settings, start a new task. The folder's trust setting will not change." => {
            "기존 작업에는 신뢰 상태에서 불러온 프로젝트 설정이나 훅을 비롯한 설정과 기록이 남아 있을 수 있습니다. 제한된 설정을 사용하려면 새 작업을 시작하세요. 폴더의 신뢰 설정은 변경되지 않습니다."
        }
        "Config, hooks, and exec policies from untrusted folders stay disabled. Trusted project folders can still contribute settings. Skills still load, and tools follow your permission settings. Opening will not change saved trust." => {
            "신뢰하지 않는 폴더의 설정, 훅, 실행 정책은 비활성화된 상태를 유지합니다. 신뢰하는 프로젝트 폴더의 설정은 적용될 수 있습니다. 스킬은 계속 불러오며 도구는 사용자의 권한 설정을 따릅니다. 폴더를 열어도 저장된 신뢰 설정은 바뀌지 않습니다."
        }
        "Trust this folder? Codex can read, edit, and run files here, subject to your permission settings. Folder settings can run code automatically, even without a model request. Continue only if you trust these files. Your trust decision will be saved." => {
            "이 폴더를 신뢰하시겠습니까? Codex는 사용자의 권한 설정에 따라 이 폴더의 파일을 읽고 수정하고 실행할 수 있습니다. 폴더 설정은 모델의 요청 없이도 코드를 자동 실행할 수 있습니다. 이 파일들을 신뢰하는 경우에만 진행하세요. 신뢰 결정이 저장됩니다."
        }
        "API key login is disabled." => "API 키 로그인이 비활성화되어 있습니다.",
        "Paste when nothing is selected, with Auto following platform defaults and On also enabling macOS" => {
            "선택한 내용이 없을 때 붙여 넣습니다. 자동은 플랫폼 기본값을 따르며 켜짐은 macOS에서도 활성화합니다."
        }
        "Use /context after the current chat is connected." => {
            "현재 대화가 연결된 뒤 /context를 사용하세요."
        }
        "default" => "기본값",
        "tokens" => "토큰",
        "unknown" => "알 수 없음",
        "This chat's context window:" => "현재 대화 컨텍스트 창:",
        "Usable window after model limits and headroom:" => {
            "모델 제한과 여유분을 반영한 사용 가능 창:"
        }
        "Changes apply to the next turn and do not increase the model's actual limit." => {
            "설정 변경은 다음 턴부터 적용되며 모델의 실제 한도를 늘리지 않습니다."
        }
        "This server does not support chat context window queries." => {
            "이 서버는 대화별 컨텍스트 창 조회를 지원하지 않습니다."
        }
        "The server did not confirm the context window setting." => {
            "서버가 컨텍스트 창 설정 적용을 확인하지 못했습니다."
        }
        "Failed to set context window:" => "컨텍스트 창 설정 실패:",
        "Usage: /context [positive integer tokens|reset]" => {
            "사용법: /context [양의 정수 토큰 수|reset]"
        }
        "Subagents" => "하위 에이전트",
        "running" => "실행 중",
        "approval needed" => "승인 대기",
        "input needed" => "입력 대기",
        "completed" => "완료",
        "failed" => "오류",
        "stopped" => "중지",
        "idle" => "대기",
        "closed" => "종료",
        "Remote-control relay connected on" => "원격 제어 relay가 연결된 기기:",
        "Remote-control relay on" => "원격 제어 relay 기기",
        "Mobile pairing and visibility are not verified by this status." => {
            "이 상태는 모바일 페어링과 기기 표시 여부를 확인하지 않습니다."
        }
        "Remote control is enabled but the readiness check timed out on" => {
            "원격 제어는 켜졌지만 연결 확인 시간이 초과된 기기:"
        }
        "No cloud environments are available for this ChatGPT workspace." => {
            "이 ChatGPT 작업 공간에서 사용할 수 있는 Cloud 환경이 없습니다."
        }
        "Not signed in. Run 'codex login' with the ChatGPT account and workspace to use Codex Cloud." => {
            "로그인하지 않았습니다. Codex Cloud를 사용하려면 해당 ChatGPT 계정과 작업 공간으로 'codex login'을 실행하세요."
        }
        "Codex Cloud requires ChatGPT sign-in; API key authentication is not supported." => {
            "Codex Cloud에는 ChatGPT 로그인이 필요합니다. API 키 인증은 지원하지 않습니다."
        }
        "Environment lookup failed" => "환경 조회 실패",
        "Invalid environment response" => "올바르지 않은 환경 응답",
        "List accessible Codex Cloud environments." => "접근 가능한 Codex Cloud 환경 목록 표시",
        "List accessible Codex Cloud environments" => "접근 가능한 Codex Cloud 환경 목록 표시",
        "Memories" => "메모리",
        "Memory scope:" => "메모리 범위:",
        "global" => "전역",
        "project" => "프로젝트",
        "State:" => "상태:",
        "Storage:" => "저장 위치:",
        "ready" => "준비됨",
        "missing" => "저장된 요약 없음",
        "disabled" => "꺼짐",
        "unavailable" => "사용 불가",
        "unreadable" => "읽기 실패",
        "oversized" => "크기 초과",
        "No stored memory summary is available." => "저장된 메모리 요약이 없습니다.",
        "This is the stored summary. Existing chat context is unchanged; new context loads the latest summary." => {
            "저장된 요약입니다. 기존 대화의 문맥은 그대로이며 새 문맥에서 최신 요약을 불러옵니다."
        }
        "Memory request failed:" => "메모리 요청 실패:",
        "Open a connected chat before managing its memories." => {
            "연결된 대화에서 메모리를 관리하세요."
        }
        "Memory correction saved for later consolidation. The stored summary and existing chats have not changed yet." => {
            "메모리 정정을 저장했습니다. 통합 대기 중이며 저장된 요약과 기존 대화는 아직 바뀌지 않았습니다."
        }
        "Memory settings saved. Reading changes apply to new chats. Generation changes stop new claims; already running work may finish." => {
            "메모리 설정을 저장했습니다. 읽기 변경은 새 대화부터 적용됩니다. 생성 변경은 새 작업 접수에 적용되며 실행 중인 작업은 완료될 수 있습니다."
        }
        "Memory scope saved for new chats. Existing global or external memories were not moved." => {
            "새 대화에 적용할 메모리 범위를 저장했습니다. 기존 전역·외부 메모리는 옮기지 않았습니다."
        }
        "Memory scope was overridden by managed configuration." => {
            "관리 설정이 메모리 범위 설정에 우선합니다."
        }
        "Usage: /memory view | edit <correction> | read on|off | generate on|off | scope project|global" => {
            "사용법: /memory view | edit <정정 내용> | read on|off | generate on|off | scope project|global"
        }
        "Project memory reset is unavailable. Disable generation and use an explicit correction; no deletion has been performed." => {
            "프로젝트 메모리 초기화는 아직 지원하지 않습니다. 생성을 끄고 명시적으로 정정하세요. 삭제는 수행하지 않았습니다."
        }
        "Local memory reset completed; existing conversations and running memory work may still retain earlier information." => {
            "로컬 메모리 초기화를 수행했습니다. 기존 대화와 실행 중인 메모리 작업에는 이전 정보가 남아 있을 수 있습니다."
        }
        "Start with remote control disabled, preserving saved registrations and settings" => {
            "저장된 등록과 설정을 유지하며 원격 제어를 끈 상태로 시작합니다"
        }
        "Start with remote control disabled, preserving saved registrations and settings." => {
            "저장된 등록과 설정을 유지하며 원격 제어를 끈 상태로 시작합니다"
        }
        "Read relay status from the existing shared daemon without starting it" => {
            "기존 공유 daemon을 새로 시작하지 않고 relay 상태를 조회합니다"
        }
        "Read relay status from the existing shared daemon without starting it." => {
            "기존 공유 daemon을 새로 시작하지 않고 relay 상태를 조회합니다"
        }
        "Emit JSON instead of plain text" => "일반 텍스트 대신 JSON 출력",
        "Emit JSON instead of plain text." => "일반 텍스트 대신 JSON 출력",
        "Emit machine-readable JSON" => "기계가 읽을 수 있는 JSON 출력",
        "Emit machine-readable JSON." => "기계가 읽을 수 있는 JSON 출력",
        "At most 100 cron jobs are allowed, including removed jobs that still own a run." => {
            "예약은 최대 100개입니다. 소유 실행이 남은 제거 예약도 포함합니다."
        }
        "The cron prompt must be nonempty, contain no NUL, and be at most 64KiB." => {
            "예약 프롬프트는 비어 있지 않은 NUL 없는 64KiB 이하 문자열이어야 합니다."
        }
        "The cron interval must be greater than zero." => "예약 간격은 0보다 커야 합니다.",
        "The scheduled time is outside the supported range." => {
            "예약 시각이 허용 범위를 넘었습니다."
        }
        "The cron interval is too large." => "예약 간격이 너무 큽니다.",
        "No more cron job IDs are available." => "예약 ID를 더 만들 수 없습니다.",
        "Finished" => "종료",
        "Target busy: skipped" => "대상 busy: 건너뜀",
        "A removed cron job cannot be resumed." => "제거한 예약은 다시 시작할 수 없습니다.",
        "The run state is unknown. It will not be retried automatically." => {
            "실행 상태가 미확인입니다. 자동 재전송하지 않습니다."
        }
        "This one-shot cron job has finished. Register a new job." => {
            "종료한 일회성 예약입니다. 새로 등록하세요."
        }
        "turn/startIfIdle unsupported: not run" => "turn/startIfIdle 미지원: 실행되지 않음",
        "The schedule must be an interval without whitespace or an RFC3339 timestamp with an offset." => {
            "일정은 공백 없는 간격 또는 offset이 포함된 RFC3339 시각이어야 합니다."
        }
        "A prompt is required to add a cron job." => "등록에는 실행할 prompt가 필요합니다.",
        "A cron job id is required." => "관리할 예약 id가 필요합니다.",
        "Use arguments for the selected action. add requires exactly one of every or at and a prompt; management actions require only an id." => {
            "action에 맞는 인자를 사용하세요. add에는 every 또는 at 하나와 prompt, 관리에는 id만 필요합니다."
        }
        "This is not a local control tool for this connection." => {
            "이 연결의 로컬 제어 도구가 아닙니다."
        }
        "The actual calling thread, turn, and call are required." => {
            "실제 호출 대화·turn·call 정보가 필요합니다."
        }
        "This thread cannot use local control tools." => {
            "이 대화는 로컬 제어 도구를 사용할 수 없습니다."
        }
        "Calling thread verification timed out: no cron request was sent." => {
            "호출 대화 확인 시간초과: 예약 요청을 보내지 않았습니다."
        }
        "Local control outcome unknown: do not retry automatically. Use list for cron jobs or the relevant query for other settings." => {
            "로컬 제어 결과 미확인: 자동 재전송하지 마세요. 예약은 list, 다른 설정은 해당 조회로 확인하세요."
        }
        "Scheduler stopped: CLI exit or connection replacement; no automatic retries" => {
            "실행기 중지: CLI 종료 또는 연결 교체, 자동 재전송 없음"
        }
        "Interval units are s, m, and h." => "간격 단위는 s, m, h입니다.",
        "The interval must be an integer greater than zero." => "간격은 0보다 큰 정수여야 합니다.",
        "The time must be an RFC3339 timestamp with an offset." => {
            "시각은 offset이 포함된 RFC3339 형식이어야 합니다."
        }
        "The scheduled time must be in the future." => "예약 시각은 미래여야 합니다.",
        "The scheduled time is too far in the future." => "예약 시각이 너무 멉니다.",
        "The cron prompt is empty." => "예약 프롬프트가 비어 있습니다.",
        "The calling thread ID is invalid." => "호출 대화 식별자가 올바르지 않습니다.",
        "The original calling thread, active turn, or connection ended or changed. The request was not executed." => {
            "원래 호출 대화·활성 turn·연결이 종료되거나 바뀌었습니다. 요청을 실행하지 않았습니다."
        }
        "The same call ID cannot be reused for a different request." => {
            "동일 호출 식별자에 다른 요청을 사용할 수 없습니다."
        }
        "The active call record limit has been reached. Do not repeat earlier requests; check after the turn ends." => {
            "동시 활성 호출 기록 한도에 도달했습니다. 이전 요청을 다시 실행하지 말고 turn 종료 후 확인하세요."
        }
        "No cron jobs are registered. Jobs disappear when the CLI exits." => {
            "등록된 예약이 없습니다. CLI를 종료하면 예약도 사라집니다."
        }
        "Ready" => "대기",
        "Waiting for admission" => "입장 응답 대기",
        "Cancellation requested; waiting for completion" => "취소 요청·종료 대기",
        "Running" => "실행 중",
        "One-shot" => "일회성",
        "No next run" => "없음",
        " (paused)" => " (일시중지)",
        " (job removed; run ownership retained)" => " (예약 제거됨; 실행 소유권 유지)",
        "There is no target thread for this cron job." => "등록 대상 대화가 없습니다.",
        "The target thread changed or is unavailable." => {
            "등록 대상 대화가 바뀌었거나 사용할 수 없습니다."
        }
        "The current thread does not allow direct input." => {
            "현재 대화는 직접 입력을 허용하지 않습니다."
        }
        "Cron is disabled because this backend does not support turn/startIfIdle." => {
            "이 backend는 turn/startIfIdle을 지원하지 않아 예약이 비활성화되어 있습니다."
        }
        "Cannot add a cron job while the backend is disconnected." => {
            "backend 연결이 끊겨 예약을 등록할 수 없습니다."
        }
        "The cron job belongs to a different backend." => "예약이 다른 backend에 속합니다.",
        "The original backend connection generation ended. Check the run state before registering a new job." => {
            "등록 당시 backend 연결 세대가 끝났습니다. 실행 상태를 확인한 뒤 새로 등록하세요."
        }
        "Cannot resume a cron job while the backend is disconnected." => {
            "backend 연결이 끊겨 예약을 재개할 수 없습니다."
        }
        "Cancellation response timed out: completion is unknown" => {
            "취소 응답 timeout: 종료 여부 미확인"
        }
        "The original backend connection generation ended." => {
            "등록 당시 backend 연결 세대가 끝났습니다."
        }
        "The cron job's owning thread or connection generation does not match." => {
            "예약의 소유 대화 또는 연결 세대가 일치하지 않습니다."
        }
        "Only a run with a confirmed owning turn can be cancelled." => {
            "정확한 소유 turn이 확인된 실행만 취소할 수 있습니다."
        }
        "The owning turn is empty or already waiting for cancellation to finish." => {
            "소유 turn이 비어 있거나 이미 취소 종료를 기다리고 있습니다."
        }
        "There is no backend connection." => "backend 연결이 없습니다.",
        "The backend generation or support status changed." => {
            "backend 세대 또는 지원 상태가 바뀌었습니다."
        }
        "Admission response timed out: execution is unknown; no retries" => {
            "입장 응답 timeout: 실행 여부 미확인, 재전송 없음"
        }
        "turn/startIfIdle unsupported" => "turn/startIfIdle 미지원",
        "This backend does not support safe cron execution. Cron jobs have been disabled." => {
            "이 backend는 안전한 예약 실행을 지원하지 않습니다. 예약을 비활성화했습니다."
        }
        "Thread closed, deleted, or archived: run state is not inferred automatically" => {
            "대화 종료·삭제·보관: 실행 상태를 자동 추정하지 않음"
        }
        "Cron thread unsubscribe timed out: detachment could not be confirmed." => {
            "예약 대화 구독 정리 timeout: 해제 여부를 확인하지 못했습니다."
        }
        "Backend event stream ended: cron execution stopped" => {
            "backend 이벤트 스트림 종료: 예약 실행 중지"
        }
        "Backend connection or events lost: cron execution stopped" => {
            "backend 연결 또는 이벤트 수신 유실: 예약 실행 중지"
        }
        "Original connection replaced: cron execution stopped" => "원래 연결 교체: 예약 실행 중지",
        "Cron job {id} was not found." => "예약 {id}를 찾을 수 없습니다.",
        "Not admitted:" => "미입장:",
        "Calling thread verification failed:" => "호출 대화 확인 실패:",
        "Connection changed; execution blocked:" => "연결 변경·실행 차단:",
        "Unsupported; execution blocked:" => "미지원·실행 차단:",
        "Outcome unknown; execution stopped:" => "미확인·실행 중지:",
        "Cron job #{id} registered: thread={thread}. Busy slots are skipped. Backend support is checked on the first dedicated request." => {
            "예약 #{id} 등록: thread={thread}. busy 회차는 건너뜁니다. backend 지원은 첫 전용 요청에서 확인합니다."
        }
        "Cancellation outcome unknown:" => "취소 결과 미확인:",
        "Cron job #{id}: {action}. remove does not cancel an already accepted turn." => {
            "예약 #{id}: {action}. remove는 이미 수락한 turn을 취소하지 않습니다."
        }
        "Cron job #{id} execution unknown: {reason}. Automatic retries have stopped." => {
            "예약 #{id} 실행 여부 미확인: {reason}. 자동 재전송을 중지했습니다."
        }
        "Cron job #{id} not admitted: {reason}. Paused." => {
            "예약 #{id} 미입장: {reason}. 일시중지했습니다."
        }
        "Cron thread unsubscribe failed:" => "예약 대화 구독 정리 실패:",
        "Calling metadata does not match." => "호출 메타데이터가 서로 다릅니다.",
        "Local control requires actual turn and call metadata." => {
            "로컬 제어에는 실제 turn·call 메타데이터가 필요합니다."
        }
        "missing task metadata" => "작업 메타데이터가 없습니다",
        "TUI is reconnecting; tool was not sent" => "TUI 재연결 중: 도구 요청을 보내지 않았습니다",
        "Local control tools are unavailable on this connection." => {
            "로컬 제어 도구가 이 연결에서 제공되지 않습니다."
        }
        "The local control request was cancelled. If the outcome is unclear, query the relevant setting and do not retry automatically." => {
            "로컬 제어 요청이 중단되었습니다. 결과가 불명확하면 해당 설정을 조회하고 자동 재전송하지 마세요."
        }
        _ => original,
    }
}

#[cfg(test)]
thread_local! {
    static TEST_LOCALE: std::cell::Cell<Option<Locale>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_locale<T>(locale: Locale, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<Locale>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_LOCALE.with(|value| value.set(self.0));
        }
    }
    let _restore = Restore(TEST_LOCALE.with(|value| value.replace(Some(locale))));
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_language_controls_ui_and_environment_override_never_writes_configuration() {
        use crate::legacy_core::config::edit::ConfigEditsBuilder;
        use codex_config::ui_language::UiLanguage;
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            locale_from_user_settings(home.path(), None).unwrap(),
            Locale::English
        );
        for (saved, expected, rendered) in [
            (UiLanguage::Korean, Locale::Korean, "작업 중"),
            (UiLanguage::English, Locale::English, "Working"),
        ] {
            ConfigEditsBuilder::new(home.path())
                .set_ui_language(saved)
                .apply_blocking()
                .unwrap();
            assert_eq!(
                locale_from_user_settings(home.path(), None).unwrap(),
                expected
            );
            assert_eq!(
                text_for(
                    locale_from_user_settings(home.path(), None).unwrap(),
                    "Working"
                ),
                rendered
            );
            let path = home.path().join("config.toml");
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(
                locale_from_user_settings(home.path(), Some("en")).unwrap(),
                Locale::English
            );
            assert_eq!(
                locale_from_user_settings(home.path(), Some("ko")).unwrap(),
                Locale::Korean
            );
            assert!(locale_from_user_settings(home.path(), Some("kr")).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn language_selection_defaults_to_english_and_rejects_unknown_values() {
        assert_eq!(Locale::from_setting(None), Ok(Locale::English));
        assert_eq!(Locale::from_setting(Some("ko")), Ok(Locale::Korean));
        assert_eq!(Locale::from_setting(Some("en")), Ok(Locale::English));
        for invalid in ["", "kr", "KO", "ja", "ko-KR"] {
            assert!(Locale::from_setting(Some(invalid)).is_err());
        }
    }

    #[test]
    fn english_and_unknown_text_preserve_the_original() {
        assert_eq!(text_for(Locale::Korean, "Working"), "작업 중");
        assert_eq!(text_for(Locale::English, "Working"), "Working");
        for token in [
            "/model",
            "read-only",
            "model",
            "request_id",
            "사용자 원문",
            "untranslated UI",
        ] {
            assert_eq!(text_for(Locale::Korean, token), token);
        }
    }

    #[test]
    fn memory_scope_labels_translate_without_changing_commands_or_paths() {
        assert_eq!(text_for(Locale::Korean, "Memory scope:"), "메모리 범위:");
        assert_eq!(text_for(Locale::Korean, "project"), "프로젝트");
        assert_eq!(text_for(Locale::English, "project"), "project");
        for opaque in [
            "/memory view",
            "thread/memory/note/add",
            "project_memories/key/memories_v2",
        ] {
            assert_eq!(text_for(Locale::Korean, opaque), opaque);
        }
    }

    #[test]
    fn scoped_language_restores_after_rendering() {
        with_locale(Locale::English, || {
            with_locale(Locale::Korean, || assert_eq!(text("Working"), "작업 중"));
            assert_eq!(text("Working"), "Working");
        });
    }
}
