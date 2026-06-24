use codex_opencode_adapter::conversion::chat_to_responses::build_response;
use codex_opencode_adapter::conversion::tool_context::ToolContext;
use codex_opencode_adapter::state::StoredResponse;
use serde_json::{json, Value};

#[test]
fn nonstream_content_with_tool_calls_outputs_only_tool_call_item() {
    let body = json!({
        "model": "opencode-go/test-model",
        "input": "What is the weather in Tokyo?",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "description": "Get weather by city.",
            "parameters": {
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                },
                "required": ["city"]
            }
        }]
    });
    let chat_response = json!({
        "id": "chatcmpl-tool-adoption",
        "object": "chat.completion",
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "I will call the weather tool now.",
                "tool_calls": [{
                    "id": "call_weather_1",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"city\":\"Tokyo\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    });
    let context = ToolContext::build(body.get("tools"));
    let mut stored: Option<StoredResponse> = None;

    let response = build_response(
        &body,
        &chat_response,
        "opencode-go/test-model",
        "test-model",
        &[],
        &context,
        |item| {
            stored = Some(item);
            Ok(())
        },
    )
    .expect("response conversion should succeed");

    let output = response
        .get("output")
        .and_then(Value::as_array)
        .expect("response output should be an array");
    assert_eq!(
        output.len(),
        1,
        "tool-call turns must not also expose assistant messages"
    );
    assert_eq!(
        output[0].get("type").and_then(Value::as_str),
        Some("function_call")
    );
    assert_eq!(
        output[0].get("call_id").and_then(Value::as_str),
        Some("call_weather_1")
    );
    assert!(
        !output
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some("message")),
        "assistant content must be suppressed from Responses output when tool_calls are present"
    );

    let stored = stored.expect("converted response should be persisted");
    let assistant = stored
        .messages
        .last()
        .expect("stored history should include the assistant tool-call message");
    assert_eq!(
        assistant.get("content").and_then(Value::as_str),
        Some("I will call the weather tool now."),
        "assistant content should remain available for replay history"
    );
    assert!(
        assistant.get("tool_calls").and_then(Value::as_array).is_some(),
        "stored history should preserve Chat tool_calls for continuation"
    );
    assert_eq!(
        stored.pending_call_ids,
        vec![String::from("call_weather_1")]
    );
}
