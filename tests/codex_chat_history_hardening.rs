use codex_opencode_adapter::codex_chat_history::{
    ensure_no_duplicate_call_outputs, validate_chat_tool_history, validate_requested_call_ids,
    HistoryResolutionError,
};
use codex_opencode_adapter::state::{now_ts, StoredResponse};
use serde_json::{json, Value};

fn stored_response(response_id: &str, pending_call_ids: Vec<&str>) -> StoredResponse {
    let tool_calls: Vec<Value> = pending_call_ids
        .iter()
        .map(|call_id| {
            json!({
                "id": call_id,
                "type": "function",
                "function": {"name": format!("tool_{call_id}"), "arguments": "{}"}
            })
        })
        .collect();

    StoredResponse {
        response_id: response_id.to_string(),
        model_alias: "opencode-go/test".to_string(),
        model_upstream: "test".to_string(),
        messages: vec![json!({"role":"assistant","content":"","tool_calls": tool_calls})],
        pending_call_ids: pending_call_ids.into_iter().map(str::to_string).collect(),
        output: Vec::new(),
        created_at: now_ts(),
        previous_response_id: String::new(),
    }
}

#[test]
fn previous_response_validation_accepts_matching_parallel_calls() {
    let previous = stored_response("resp_1", vec!["call_weather", "call_time"]);

    validate_requested_call_ids(
        &previous,
        &["call_weather".to_string(), "call_time".to_string()],
    )
    .unwrap();
}

#[test]
fn previous_response_validation_rejects_unknown_call_id() {
    let previous = stored_response("resp_1", vec!["call_1"]);

    let err = validate_requested_call_ids(&previous, &["call_unknown".to_string()]).unwrap_err();

    assert!(matches!(
        err,
        HistoryResolutionError::UnknownToolCall { .. }
    ));
}

#[test]
fn duplicate_tool_outputs_are_rejected_before_history_repair() {
    let err = ensure_no_duplicate_call_outputs(&["call_1".to_string(), "call_1".to_string()])
        .unwrap_err();

    assert!(matches!(
        err,
        HistoryResolutionError::DuplicateToolOutput { .. }
    ));
}

#[test]
fn chat_history_accepts_tool_output_after_assistant_tool_call() {
    let messages = vec![
        json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{}"}
            }]
        }),
        json!({
            "role": "tool",
            "tool_call_id": "call_1",
            "content": "sunny"
        }),
    ];

    validate_chat_tool_history(&messages).unwrap();
}

#[test]
fn chat_history_rejects_orphan_tool_output() {
    let messages = vec![json!({
        "role": "tool",
        "tool_call_id": "call_1",
        "content": "sunny"
    })];

    let err = validate_chat_tool_history(&messages).unwrap_err();

    assert!(matches!(
        err,
        HistoryResolutionError::InvalidToolHistory { .. }
    ));
}

#[test]
fn chat_history_rejects_duplicate_tool_output() {
    let messages = vec![
        json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{}"}
            }]
        }),
        json!({"role": "tool", "tool_call_id": "call_1", "content": "sunny"}),
        json!({"role": "tool", "tool_call_id": "call_1", "content": "sunny again"}),
    ];

    let err = validate_chat_tool_history(&messages).unwrap_err();

    assert!(matches!(
        err,
        HistoryResolutionError::DuplicateToolOutput { .. }
    ));
}
