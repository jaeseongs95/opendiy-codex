// Modified for OpenDIY Codex R32 (2026-11-01); see MODIFICATIONS.md.
use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

/// Read only direct children. Responses contain no conversation or filesystem data.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct ThreadSubagentsListParams {
    pub parent_thread_id: String,
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
    #[ts(optional = nullable)]
    pub reconcile_request_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadSubagentsListResponse {
    pub parent_thread_id: String,
    pub parent_runtime_id: String,
    pub agents: Vec<SubagentManagementAgent>,
    pub next_cursor: Option<String>,
    pub mutation_state: SubagentMutationState,
    pub reconciled_request_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SubagentManagementAgent {
    pub id: String,
    pub parent_thread_id: String,
    #[schemars(schema_with = "required_nullable_runtime_schema")]
    pub runtime_id: Option<String>,
    pub nickname: Option<String>,
    pub role: Option<String>,
    pub status: SubagentManagementStatus,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub active_turn_id: Option<String>,
    pub capabilities: SubagentManagementCapabilities,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SubagentManagementCapabilities {
    pub send: bool,
    pub interrupt: bool,
    pub close: bool,
    pub resume: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SubagentManagementStatus {
    Pending,
    Running,
    Idle,
    Completed,
    Errored,
    Closed,
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SubagentMutationState {
    Clear,
    Unknown,
}

/// An operator command, admitted through the authenticated app-server connection.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export_to = "v2/")]
pub struct ThreadSubagentsControlParams {
    pub parent_thread_id: String,
    pub expected_parent_runtime_id: String,
    pub agent_id: String,
    // Explicit null means the caller observed an unloaded child. Omission is invalid.
    #[serde(deserialize_with = "required_nullable_runtime")]
    #[schemars(schema_with = "required_nullable_runtime_schema")]
    pub expected_runtime_id: Option<String>,
    pub action: SubagentManagementAction,
    pub request_id: String,
    #[ts(optional = nullable)]
    pub text: Option<String>,
    #[ts(optional = nullable)]
    pub expected_turn_id: Option<String>,
}

fn required_nullable_runtime<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

// schema_with supplies a required field while preserving Option's nullable value schema.
#[cfg(test)]
fn required_nullable_runtime_schema(
    generator: &mut schemars::r#gen::SchemaGenerator,
) -> schemars::schema::Schema {
    generator.subschema_for::<Option<String>>()
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SubagentManagementAction {
    Send,
    Interrupt,
    Close,
    Resume,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ThreadSubagentsControlResponse {
    pub status: SubagentControlStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub enum SubagentControlStatus {
    /// Accepted by the runtime; this does not mean the agent completed the instruction.
    Applied,
    NotApplied,
    Unknown,
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn control_requires_explicit_runtime_observation() {
        let mut request = json!({
            "parentThreadId": "parent", "expectedParentRuntimeId": "runtime",
            "agentId": "child", "action": "resume", "requestId": "request"
        });
        assert!(serde_json::from_value::<ThreadSubagentsControlParams>(request.clone()).is_err());
        request["expectedRuntimeId"] = serde_json::Value::Null;
        let parsed = serde_json::from_value::<ThreadSubagentsControlParams>(request).unwrap();
        assert_eq!(
            serde_json::to_value(parsed)
                .unwrap()
                .get("expectedRuntimeId"),
            Some(&serde_json::Value::Null)
        );
    }

    #[test]
    fn control_schema_and_typescript_require_a_nullable_runtime() {
        let schema =
            serde_json::to_value(schemars::schema_for!(ThreadSubagentsControlParams)).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("expectedRuntimeId")));
        let types = schema["properties"]["expectedRuntimeId"]["type"]
            .as_array()
            .unwrap();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&json!("string")));
        assert!(types.contains(&json!("null")));
        let declaration = <ThreadSubagentsControlParams as ts_rs::TS>::decl();
        assert!(declaration.contains("expectedRuntimeId: string | null"));
        assert!(!declaration.contains("expectedRuntimeId?:"));
    }

    #[test]
    fn agent_schema_and_output_require_a_nullable_runtime() {
        let schema = serde_json::to_value(schemars::schema_for!(SubagentManagementAgent)).unwrap();
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("runtimeId"))
        );
        let types = schema["properties"]["runtimeId"]["type"]
            .as_array()
            .unwrap();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&json!("string")));
        assert!(types.contains(&json!("null")));
        let declaration = <SubagentManagementAgent as ts_rs::TS>::decl();
        assert!(declaration.contains("runtimeId: string | null"));
        assert!(!declaration.contains("runtimeId?:"));

        // Response decoding retains Option's tolerance; wire output always carries runtimeId.
        let decoded: SubagentManagementAgent = serde_json::from_value(json!({
            "id": "child",
            "parentThreadId": "parent",
            "status": "unknown",
            "capabilities": { "send": false, "interrupt": false, "close": false, "resume": true }
        }))
        .unwrap();
        assert!(decoded.runtime_id.is_none());
        assert_eq!(
            serde_json::to_value(decoded).unwrap().get("runtimeId"),
            Some(&serde_json::Value::Null)
        );
    }

    #[test]
    fn response_optional_values_are_explicit_nulls_and_required_in_typescript() {
        let agent: SubagentManagementAgent = serde_json::from_value(json!({
            "id": "child", "parentThreadId": "parent", "status": "unknown",
            "capabilities": { "send": false, "interrupt": false, "close": false, "resume": true }
        }))
        .unwrap();
        let output = serde_json::to_value(agent).unwrap();
        let declaration = <SubagentManagementAgent as ts_rs::TS>::decl();
        for key in [
            "runtimeId",
            "nickname",
            "role",
            "model",
            "effort",
            "activeTurnId",
        ] {
            assert_eq!(output.get(key), Some(&serde_json::Value::Null), "{key}");
            assert!(
                declaration.contains(&format!("{key}: string | null")),
                "{key}"
            );
            assert!(!declaration.contains(&format!("{key}?:")), "{key}");
        }
        let response = ThreadSubagentsListResponse {
            parent_thread_id: "parent".into(),
            parent_runtime_id: "runtime".into(),
            agents: vec![],
            next_cursor: None,
            mutation_state: SubagentMutationState::Clear,
            reconciled_request_id: None,
        };
        assert_eq!(
            serde_json::to_value(response)
                .unwrap()
                .get("reconciledRequestId"),
            Some(&serde_json::Value::Null)
        );
        let declaration = <ThreadSubagentsListResponse as ts_rs::TS>::decl();
        assert!(declaration.contains("reconciledRequestId: string | null"));
        assert!(!declaration.contains("reconciledRequestId?:"));
    }

    #[test]
    fn status_does_not_serialize_agent_result() {
        assert_eq!(
            serde_json::to_value(SubagentManagementStatus::Completed).unwrap(),
            "completed"
        );
        assert_eq!(
            serde_json::to_value(SubagentControlStatus::NotApplied).unwrap(),
            "notApplied"
        );
    }
}
