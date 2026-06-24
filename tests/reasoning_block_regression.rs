use codex_opencode_adapter::conversion::{build_chat_payload, build_response, StreamAssembler};
use codex_opencode_adapter::state::StoredResponse;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const OPEN_TAG: &str = concat!("<", "think", ">");
const CLOSE_TAG: &str = concat!("</", "think", ">");

fn tagged_content(reasoning: &str, answer: &str) -> String {
    format!("{OPEN_TAG}{reasoning}{CLOSE_TAG}\n{answer}")
}

#[test]
fn nonstream_leading_reasoning_block_becomes_reasoning_item() {
    let body = json!({
        "model": "opencode-go/test-model",
        "input": "answer with reasoning",
        "stream": false
    });
    let (_payload, messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "test-model", None, json!({})).unwrap();
    let upstream = json!({
        "id": "chatcmpl_reasoning_block",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": tagged_content("hidden chain", "visible answer")
            },
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });
    let mut stored = Vec::<StoredResponse>::new();

    let response = build_response(
        &body,
        &upstream,
        "opencode-go/test-model",
        "test-model",
        &messages,
        &tool_ctx,
        |item| {
            stored.push(item);
            Ok(())
        },
    )
    .unwrap();

    let output = response["output"].as_array().unwrap();
    let reasoning = output
        .iter()
        .find(|item| item["type"] == "reasoning")
        .unwrap();
    assert_eq!(reasoning["summary"][0]["text"], "hidden chain");

    let message = output
        .iter()
        .find(|item| item["type"] == "message")
        .unwrap();
    assert_eq!(message["content"][0]["text"], "visible answer");
    assert!(!message["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("hidden chain"));
    assert_eq!(stored[0].messages.last().unwrap()["content"], "visible answer");
    assert_eq!(
        stored[0].messages.last().unwrap()["reasoning_content"],
        "hidden chain"
    );
}

#[test]
fn stream_leading_reasoning_block_is_not_emitted_as_output_text() {
    let body = json!({
        "model": "opencode-go/test-model",
        "input": "answer with reasoning",
        "stream": true
    });
    let (_payload, messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "test-model", None, json!({})).unwrap();
    let events = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let stored = Arc::new(Mutex::new(Vec::<StoredResponse>::new()));
    let events_for_emit = Arc::clone(&events);
    let stored_for_put = Arc::clone(&stored);
    let mut assembler = StreamAssembler::new(
        body,
        "opencode-go/test-model".to_string(),
        "test-model".to_string(),
        messages,
        tool_ctx,
        Box::new(move |item| {
            stored_for_put.lock().unwrap().push(item);
            Ok(())
        }),
        Box::new(move |event, data| {
            events_for_emit
                .lock()
                .unwrap()
                .push((event.to_string(), data));
            Ok(())
        }),
    );

    assembler.start().unwrap();
    assembler
        .accept(&json!({"choices":[{"delta":{"content":"<th"},"finish_reason":null}]}))
        .unwrap();
    assembler
        .accept(&json!({"choices":[{"delta":{"content":"ink>hidden chain</thi"},"finish_reason":null}]}))
        .unwrap();
    assembler
        .accept(&json!({"choices":[{"delta":{"content":"nk>\nvisible answer"},"finish_reason":"stop"}]}))
        .unwrap();
    let response = assembler.finalize().unwrap();

    let events = events.lock().unwrap();
    let reasoning_text = events
        .iter()
        .filter(|(name, _)| name == "response.reasoning_summary_text.delta")
        .filter_map(|(_, data)| data["delta"].as_str())
        .collect::<String>();
    assert_eq!(reasoning_text, "hidden chain");

    let output_text = events
        .iter()
        .filter(|(name, _)| name == "response.output_text.delta")
        .filter_map(|(_, data)| data["delta"].as_str())
        .collect::<String>();
    assert_eq!(output_text, "visible answer");
    assert!(!output_text.contains("hidden chain"));
    assert!(!output_text.contains("think"));

    let output = response["output"].as_array().unwrap();
    assert!(output.iter().any(|item| item["type"] == "reasoning"));
    let message = output
        .iter()
        .find(|item| item["type"] == "message")
        .unwrap();
    assert_eq!(message["content"][0]["text"], "visible answer");
}
