use std::sync::{Arc, Mutex};

use codex_opencode_adapter::conversion::responses_to_chat::{
    build_chat_payload, function_output_call_ids,
};
use codex_opencode_adapter::conversion::stream_chat_to_responses::StreamAssembler;
use codex_opencode_adapter::conversion::tool_context::ToolContext;
use serde_json::{json, Value};

#[test]
fn function_output_call_ids_reads_responses_call_id() {
    let body = json!({
        "model": "opencode-go/test-model",
        "input": [
            {"type": "function_call_output", "call_id": "call_123", "output": "ok"},
            {"type": "custom_tool_call_output", "call_id": "call_456", "output": "done"}
        ]
    });

    let ids = function_output_call_ids(&body).expect("extract call ids");
    assert_eq!(ids, vec!["call_123".to_string(), "call_456".to_string()]);
}

#[test]
fn build_chat_payload_converts_custom_and_tool_search_calls() {
    let body = json!({
        "model": "opencode-go/test-model",
        "tools": [
            {"type": "custom", "name": "shell", "description": "run shell"},
            {"type": "tool_search"}
        ],
        "input": [
            {"type": "custom_tool_call", "call_id": "call_custom", "name": "shell", "input": "ls -la"},
            {"type": "tool_search_call", "call_id": "call_search", "arguments": {"query": "gmail"}}
        ]
    });

    let (_payload, messages, _reverse, _context) =
        build_chat_payload(&body, "test-model", None, json!({})).expect("build payload");

    let tool_calls = messages
        .iter()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(Value::as_array)
        .expect("assistant tool calls");

    assert_eq!(tool_calls[0]["id"], "call_custom");
    assert_eq!(tool_calls[0]["function"]["name"], "shell");
    assert_eq!(
        tool_calls[0]["function"]["arguments"],
        r#"{"input":"ls -la"}"#
    );
    assert_eq!(tool_calls[1]["id"], "call_search");
    assert_eq!(tool_calls[1]["function"]["name"], "tool_search");
    assert_eq!(
        tool_calls[1]["function"]["arguments"],
        r#"{"query":"gmail"}"#
    );
}

#[test]
fn stream_truncated_with_output_can_finalize_as_incomplete() {
    let stored = Arc::new(Mutex::new(Vec::new()));
    let stored_for_put = stored.clone();
    let emitted = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let emitted_for_put = emitted.clone();

    let mut assembler = StreamAssembler::new(
        json!({"model": "opencode-go/test-model", "stream": true}),
        "opencode-go/test-model".to_string(),
        "test-model".to_string(),
        vec![],
        ToolContext::build(None),
        Box::new(move |item| {
            stored_for_put.lock().expect("stored lock").push(item);
            Ok(())
        }),
        Box::new(move |event, payload| {
            emitted_for_put
                .lock()
                .expect("emitted lock")
                .push((event.to_string(), payload));
            Ok(())
        }),
    );

    assembler.start().expect("start stream");
    assembler
        .accept(&json!({
            "choices": [{"delta": {"content": "hello"}}]
        }))
        .expect("accept content");
    assert!(assembler.has_substantive_output());
    assert!(!assembler.has_finish_reason());

    assembler.mark_truncated_as_length();
    let response = assembler.finalize().expect("finalize response");

    assert_eq!(response["status"], "incomplete");
    assert_eq!(
        response["incomplete_details"]["reason"],
        "max_output_tokens"
    );
    assert_eq!(
        emitted.lock().expect("emitted lock").last().unwrap().0,
        "response.incomplete"
    );
    assert_eq!(stored.lock().expect("stored lock").len(), 1);
}
