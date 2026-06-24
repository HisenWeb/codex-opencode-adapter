#[path = "../src/codex_chat_history.rs"]
mod codex_chat_history;

use codex_chat_history::{
    resolve_history, validate_chat_tool_history, HistoryResolutionError,
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
fn previous_response_id_takes_priority_over_ambiguous_call_id() {
    let responses = vec![
        stored_response("resp_1", vec!["call_1"]),
        stored_response("resp_2", vec!["call_1"]),
    ];

    let resolved = resolve_history(
        Some("resp_1"),
        &["call_1".to_string()],
        &responses,
    )
    .unwrap()
    .unwrap();

    assert_eq!(resolved.response_id, "resp_1");
}

#[test]
fn unique_call_id_fallback_restores_matching_response() {
    let responses = vec![stored_response("resp_1", vec!["call_1"])] ;

    let resolved = resolve_history(None, &["call_1".to_string()], &responses)
        .unwrap()
        .unwrap();

    assert_eq!(resolved.response_id, "resp_1");
}

#[test]
fn ambiguous_call_id_without_previous_response_returns_error() {
    let responses = vec![
        stored_response("resp_1", vec!["call_1"]),
        stored_response("resp_2", vec!["call_1"]),
    ];

    let err = resolve_history(None, &["call_1".to_string()], &responses).unwrap_err();

    assert!(matches!(
        err,
        HistoryResolutionError::AmbiguousToolCall { .. }
    ));
}

#[test]
fn parallel_tool_outputs_from_same_response_are_allowed() {
    let responses = vec![stored_response("resp_1", vec!["call_weather", "call_time"])] ;

    let resolved = resolve_history(
        None,
        &["call_weather".to_string(), "call_time".to_string()],
        &responses,
    )
    .unwrap()
    .unwrap();

    assert_eq!(resolved.response_id, "resp_1");
}

#[test]
fn parallel_tool_outputs_split_across_responses_are_rejected() {
    let responses = vec![
        stored_response("resp_1", vec!["call_weather"]),
        stored_response("resp_2", vec!["call_time"]),
    ];

    let err = resolve_history(
        None,
        &["call_weather".to_string(), "call_time".to_string()],
        &responses,
    )
    .unwrap_err();

    assert!(matches!(
        err,
        HistoryResolutionError::SplitToolOutputs { .. }
    ));
}

#[test]
fn unknown_call_id_returns_error() {
    let responses = vec![stored_response("resp_1", vec!["call_1"])] ;

    let err = resolve_history(None, &["call_unknown".to_string()], &responses).unwrap_err();

    assert!(matches!(
        err,
        HistoryResolutionError::UnknownToolCall { .. }
    ));
}

#[test]
fn duplicate_tool_outputs_return_error() {
    let responses = vec![stored_response("resp_1", vec!["call_1"])] ;

    let err = resolve_history(
        None,
        &["call_1".to_string(), "call_1".to_string()],
        &responses,
    )
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
