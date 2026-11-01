// Modified for OpenDIY Codex R31 (2026-11-01); see MODIFICATIONS.md.
//! Structured model entry into the existing client settings and thread controls.
use codex_app_server_protocol::DynamicToolFunctionSpec;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) const NAME: &str = "session_control";
const PREFIX: &str = "session_control:";

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    Status,
    MemoryView,
    MemoryNote { note: String },
    MemoryForget { note: String },
    MemoryRead { enabled: bool },
    MemoryGenerate { enabled: bool },
    MemoryScope { scope: Scope },
    ContextGet,
    ContextSet { tokens: i64 },
    ContextReset,
    UiLanguage { language: String },
    ServiceTier { tier: String },
    ReloadPlugins,
    CleanupAgentsPreview,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Scope {
    Project,
    Global,
}

pub(crate) fn parse(value: Value) -> Result<Command, String> {
    let command: Command = serde_json::from_value(value).map_err(|error| error.to_string())?;
    match &command {
        Command::MemoryNote { note } | Command::MemoryForget { note }
            if note.trim().is_empty() || note.len() > 16 * 1024 =>
        {
            return Err(
                "메모리 정정은 비어 있지 않은 16 KiB 이하의 사용자 요청이어야 합니다.".into(),
            );
        }
        Command::ContextSet { tokens } if *tokens <= 0 => {
            return Err("문맥 크기는 양의 정수 토큰 수여야 합니다.".into());
        }
        Command::UiLanguage { language } => {
            codex_config::ui_language::UiLanguage::parse(language)
                .map_err(|error| error.to_string())?;
        }
        Command::ServiceTier { tier } if tier.trim().is_empty() => {
            return Err("status에서 확인한 지원 속도 식별자가 필요합니다.".into());
        }
        _ => {}
    }
    Ok(command)
}

pub(crate) fn command(value: Value) -> Result<String, String> {
    serde_json::to_string(&parse(value)?)
        .map(|json| format!("{PREFIX}{json}"))
        .map_err(|error| error.to_string())
}

pub(crate) fn from_command(command: &str) -> Result<Command, String> {
    let json = command
        .strip_prefix(PREFIX)
        .ok_or("로컬 제어 요청 형식이 잘못되었습니다.")?;
    parse(serde_json::from_str(json).map_err(|error| error.to_string())?)
}

pub(crate) fn is_read_only_command(command: &str) -> bool {
    matches!(
        from_command(command),
        Ok(Command::Status
            | Command::MemoryView
            | Command::ContextGet
            | Command::CleanupAgentsPreview)
    )
}

pub(crate) fn spec() -> DynamicToolFunctionSpec {
    DynamicToolFunctionSpec {
        name: NAME.into(),
        description: "사용자가 이 앱의 기능을 자연어로 요청하면 실행하는 로컬 제어 도구입니다. 메모리 보여줘/기억해/이 기억 정정해/잊어줘는 memory_view/memory_note/memory_forget, 메모리 읽기·생성 켜기/끄기는 memory_read/memory_generate(enabled), 프로젝트/전역 메모리는 memory_scope(scope), 이 대화 문맥 크기 조회·변경·초기화는 context_get/context_set(tokens)/context_reset, 화면 언어 한국어/영어로 바꿔줘는 ui_language(ko/en), 빠르게/ultrafast로 바꿔줘는 status로 현재 지원 tier를 확인한 뒤 service_tier(tier), 플러그인 다시 읽어줘는 reload_plugins를 사용하세요. slash 명령 입력을 사용자에게 떠넘기거나 셸로 설정·메모리 파일을 직접 고치지 마세요. 메모리 정정·망각은 후속 통합용 기록이며 요약·기존 대화가 즉시 바뀌거나 영구 삭제된 것이 아닙니다. 읽기·범위 설정은 새 대화, 생성 변경은 새 처리에 적용되며 이미 실행 중인 처리는 끝날 수 있습니다. 문맥 변경은 현재 호출 대화의 다음 턴부터이며 모델 한도를 늘리지 않습니다. 화면 언어는 클라이언트 전역에 저장되어 다음 실행에 적용되고 환경변수가 우선합니다. 속도는 현재 모델이 지원하는 식별자만 사용하며 현재 대화에만 적용합니다. 플러그인 응답은 다시 연결 요청의 확인이며 서버 준비 완료가 아닙니다. 종료된 하위 작업 정리는 cleanup_agents_preview로 현재 대화 아래 후보만 조회하세요. 이 동작은 삭제하지 않으며 삭제 완료라고 말하지 마세요. 실제 영구 삭제는 미리보기의 정확한 대상과 기존 cleanup-agents 명시 확인 절차를 유지하세요. 이 도구에는 confirmed/force/임의 threadId/경로 입력이 없습니다. 예약은 별도 cron 도구를 사용하세요. 사용자의 명시 요청·정정을 보존하고 모호한 변경은 먼저 질문하세요. 실제 성공 응답 전 완료를 말하지 말고 시간초과·유실 뒤 변경을 자동 재전송하지 마세요.".into(),
        input_schema: json!({
            "type":"object", "additionalProperties":false,
            "properties": {
                "action":{"type":"string","enum":["status","memory_view","memory_note","memory_forget","memory_read","memory_generate","memory_scope","context_get","context_set","context_reset","ui_language","service_tier","reload_plugins","cleanup_agents_preview"]},
                "note":{"type":"string","minLength":1,"description":"사용자가 명시한 추가·정정·망각 내용. 최대 16 KiB."},
                "enabled":{"type":"boolean"},
                "scope":{"type":"string","enum":["project","global"]},
                "tokens":{"type":"integer","minimum":1},
                "language":{"type":"string","enum":["ko","en"]},
                "tier":{"type":"string","minLength":1,"description":"status에서 확인한 현재 모델의 지원 속도 식별자 또는 default"}
            }, "required":["action"]
        }),
        defer_loading: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_controls_reject_foreign_targets_and_destructive_confirmation() {
        for value in [
            json!({"action":"memory_note","note":" "}),
            json!({"action":"context_set","tokens":0}),
            json!({"action":"context_set","tokens":1,"threadId":"other"}),
            json!({"action":"memory_scope","scope":"external"}),
            json!({"action":"ui_language","language":"invalid"}),
            json!({"action":"service_tier","tier":" "}),
            json!({"action":"cleanup_agents_preview","confirmed":true}),
            json!({"action":"cleanup_agents_preview","force":true}),
            json!({"action":"memory_reset"}),
            json!({"action":"reload_plugins","path":"other"}),
        ] {
            assert!(command(value).is_err());
        }
        for value in [
            json!({"action":"memory_forget","note":"이전 선호"}),
            json!({"action":"memory_generate","enabled":false}),
            json!({"action":"context_set","tokens":120000}),
            json!({"action":"ui_language","language":"ko"}),
            json!({"action":"cleanup_agents_preview"}),
        ] {
            let expected = parse(value.clone()).unwrap();
            assert_eq!(from_command(&command(value).unwrap()).unwrap(), expected);
        }
        assert!(is_read_only_command(
            &command(json!({"action":"memory_view"})).unwrap()
        ));
        assert!(!is_read_only_command(
            &command(json!({"action":"memory_note","note":"정정"})).unwrap()
        ));
    }
}
