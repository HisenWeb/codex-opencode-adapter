use codex_opencode_adapter::conversion::{build_chat_payload, function_output_call_ids};
use codex_opencode_adapter::state::StoredResponse;
use serde_json::{json, Value};

#[test]
fn stateless_full_history_tool_output_builds_chat_payload() {
    let body = json!({
        "model": "opencode-go/test-model",
        "input": [
            {"type":"message","role":"user","content":"call a tool"},
            {"type":"function_call","call_id":"call_1","name":"run","arguments":"{\"cmd\":\"echo ok\"}"},
            {"type":"function_call_output","call_id":"call_1","output":"ok"},
            {"type":"message","role":"user","content":"continue"}
        ]
    });

    assert!(function_output_call_ids(&body).unwrap().is_empty());

    let (payload, messages, _reverse_names, _tool_ctx) =
        build_chat_payload(&body, "test-model", None, json!({})).unwrap();

    let chat_messages = payload["messages"].as_array().unwrap();
    assert!(chat_messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("assistant")
            && message
                .get("tool_calls")
                .and_then(Value::as_array)
                .is_some()
    }));
    assert!(chat_messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("tool")
            && message.get("tool_call_id").and_then(Value::as_str) == Some("call_1")
    }));
    assert_eq!(messages, chat_messages.clone());
}

#[test]
fn stateless_orphan_tool_output_still_fails_without_state() {
    let body = json!({
        "model": "opencode-go/test-model",
        "input": [
            {"type":"function_call_output","call_id":"call_1","output":"ok"}
        ]
    });

    assert_eq!(
        function_output_call_ids(&body).unwrap(),
        vec!["call_1".to_string()]
    );

    let error = build_chat_payload(&body, "test-model", None, json!({}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("tool output has no matching stored response"));
}

#[test]
fn stateless_duplicate_tool_outputs_are_rejected_by_repair() {
    let body = json!({
        "model": "opencode-go/test-model",
        "input": [
            {"type":"function_call","call_id":"call_1","name":"run","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"one"},
            {"type":"function_call_output","call_id":"call_1","output":"two"}
        ]
    });

    assert!(function_output_call_ids(&body).unwrap().is_empty());

    let error = build_chat_payload(&body, "test-model", None, json!({}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate tool output"));
}

#[test]
fn previous_response_id_does_not_bypass_state_lookup_even_with_full_history() {
    let body = json!({
        "previous_response_id": "resp_prev",
        "model": "opencode-go/test-model",
        "input": [
            {"type":"function_call","call_id":"call_1","name":"run","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"ok"}
        ]
    });

    assert_eq!(
        function_output_call_ids(&body).unwrap(),
        vec!["call_1".to_string()]
    );
}

#[test]
fn previous_response_full_history_does_not_duplicate_replayed_tool_call() {
    let previous = StoredResponse {
        response_id: "resp_prev".to_string(),
        model_alias: "opencode-go/test-model".to_string(),
        model_upstream: "test-model".to_string(),
        messages: vec![
            json!({"role":"user","content":"call a tool"}),
            json!({
                "role":"assistant",
                "content":"",
                "tool_calls":[{
                    "id":"call_1",
                    "type":"function",
                    "function":{"name":"run","arguments":"{}"}
                }]
            }),
        ],
        pending_call_ids: vec!["call_1".to_string()],
        output: vec![],
        created_at: 0,
        previous_response_id: String::new(),
    };

    let body = json!({
        "previous_response_id": "resp_prev",
        "model": "opencode-go/test-model",
        "input": [
            {"type":"message","role":"user","content":"call a tool"},
            {"type":"function_call","call_id":"call_1","name":"run","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"ok"},
            {"type":"message","role":"user","content":"continue"}
        ]
    });

    let (_payload, messages, _reverse_names, _tool_ctx) =
        build_chat_payload(&body, "test-model", Some(&previous), json!({})).unwrap();

    let assistant_calls = messages
        .iter()
        .filter(|message| {
            message.get("role").and_then(Value::as_str) == Some("assistant")
                && message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| {
                        calls
                            .iter()
                            .any(|call| call.get("id").and_then(Value::as_str) == Some("call_1"))
                    })
        })
        .count();
    let tool_outputs = messages
        .iter()
        .filter(|message| {
            message.get("role").and_then(Value::as_str) == Some("tool")
                && message.get("tool_call_id").and_then(Value::as_str) == Some("call_1")
        })
        .count();

    assert_eq!(
        assistant_calls, 1,
        "call_1 should only appear once as a replayed assistant tool_call"
    );
    assert_eq!(
        tool_outputs, 1,
        "call_1 should only appear once as a tool output"
    );
    assert_eq!(
        messages
            .last()
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str),
        Some("continue")
    );
}
