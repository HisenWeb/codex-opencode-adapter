use codex_opencode_adapter::conversion::responses_to_chat::build_chat_payload;
use codex_opencode_adapter::conversion::{build_response, StreamAssembler};
use codex_opencode_adapter::state::StoredResponse;
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};

#[test]
fn tool_search_uses_query_limit_schema() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Find a tool",
        "tools": [{"type":"tool_search"}],
        "stream": true
    });
    let (payload, _messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-pro", None, json!({})).unwrap();
    let tool = &payload["tools"][0]["function"];
    assert_eq!(tool["name"], "tool_search");
    assert_eq!(tool["parameters"]["properties"]["query"]["type"], "string");
    assert_eq!(tool["parameters"]["properties"]["limit"]["type"], "integer");
    assert_eq!(tool["parameters"]["required"], json!(["query"]));
    assert!(tool["parameters"]["properties"].get("input").is_none());
    assert!(tool_ctx.is_tool_search_chat_name("tool_search"));
    assert!(!tool_ctx.is_custom_tool_chat_name("tool_search"));
}

#[test]
fn nonstream_tool_search_restores_response_item() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Find a tool",
        "tools": [{"type":"tool_search"}],
        "stream": false
    });
    let (_payload, messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-pro", None, json!({})).unwrap();

    let mut message = Map::new();
    message.insert("role".to_string(), json!("assistant"));
    message.insert("content".to_string(), Value::Null);
    message.insert(
        tool_calls_key(),
        json!([{
            "id": "call_search",
            "type": "function",
            "function": {
                "name": "tool_search",
                "arguments": "{\"query\":\"filesystem\",\"limit\":3}"
            }
        }]),
    );

    let upstream = json!({
        "id": "chatcmpl_test",
        "choices": [{"message": Value::Object(message), "finish_reason": "tool_calls"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });
    let mut stored = Vec::<StoredResponse>::new();
    let response = build_response(
        &body,
        &upstream,
        "opencode-go/deepseek-v4-pro",
        "deepseek-v4-pro",
        &messages,
        &tool_ctx,
        |item| {
            stored.push(item);
            Ok(())
        },
    )
    .unwrap();
    let output = response["output"].as_array().unwrap();
    let item = output
        .iter()
        .find(|item| item["type"] == "tool_search_call")
        .unwrap();
    assert_eq!(item["call_id"], "call_search");
    assert_eq!(item["arguments"]["query"], "filesystem");
    assert_eq!(item["arguments"]["limit"], 3);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].pending_call_ids, vec!["call_search".to_string()]);
}

#[test]
fn streaming_tool_search_does_not_emit_custom_input_events() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Find a tool",
        "tools": [{"type":"tool_search"}],
        "stream": true
    });
    let (_payload, messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-pro", None, json!({})).unwrap();
    let events = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let stored = Arc::new(Mutex::new(Vec::<StoredResponse>::new()));
    let events_for_emit = Arc::clone(&events);
    let stored_for_put = Arc::clone(&stored);
    let mut assembler = StreamAssembler::new(
        body,
        "opencode-go/deepseek-v4-pro".to_string(),
        "deepseek-v4-pro".to_string(),
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
        .accept(&stream_tool_chunk(
            0,
            Some("call_search"),
            Some("tool_search"),
            Some("{\"query\":"),
            None,
        ))
        .unwrap();
    assembler
        .accept(&stream_tool_chunk(
            0,
            None,
            None,
            Some("\"filesystem\"}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();
    let events = events.lock().unwrap();
    assert!(!events
        .iter()
        .any(|(name, _)| name.starts_with("response.custom_tool_call_input")));
    assert!(events
        .iter()
        .any(|(name, _)| name == "response.function_call_arguments.done"));
    let done = events
        .iter()
        .find(|(name, _)| name == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(done.1["arguments"], "{\"query\":\"filesystem\"}");
    let output = response["output"].as_array().unwrap();
    let item = output
        .iter()
        .find(|item| item["type"] == "tool_search_call")
        .unwrap();
    assert_eq!(item["arguments"]["query"], "filesystem");
}

#[test]
fn streaming_custom_tool_buffers_split_arguments_until_finalize() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Record a note",
        "tools": [{"type":"custom","name":"custom.echo"}],
        "stream": true
    });
    let (_payload, messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-pro", None, json!({})).unwrap();
    let events = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let stored = Arc::new(Mutex::new(Vec::<StoredResponse>::new()));
    let events_for_emit = Arc::clone(&events);
    let stored_for_put = Arc::clone(&stored);
    let mut assembler = StreamAssembler::new(
        body,
        "opencode-go/deepseek-v4-pro".to_string(),
        "deepseek-v4-pro".to_string(),
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
    let first = format!("{{\"{}\":", "input");
    assembler
        .accept(&stream_tool_chunk(
            0,
            Some("call_custom"),
            Some("custom_echo"),
            Some(&first),
            None,
        ))
        .unwrap();
    {
        let events = events.lock().unwrap();
        assert!(!events
            .iter()
            .any(|(name, _)| name == "response.custom_tool_call_input.delta"));
    }
    assembler
        .accept(&stream_tool_chunk(
            0,
            None,
            None,
            Some("\"hello world\"}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();
    let events = events.lock().unwrap();
    let deltas = events
        .iter()
        .filter(|(name, _)| name == "response.custom_tool_call_input.delta")
        .collect::<Vec<_>>();
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].1["delta"], "hello world");
    let done = events
        .iter()
        .find(|(name, _)| name == "response.custom_tool_call_input.done")
        .unwrap();
    assert_eq!(done.1["input"], "hello world");
    let output = response["output"].as_array().unwrap();
    let item = output
        .iter()
        .find(|item| item["type"] == "custom_tool_call")
        .unwrap();
    assert_eq!(item["input"], "hello world");
}

fn tool_calls_key() -> String {
    ["tool", "calls"].join("_")
}

fn stream_tool_chunk(
    index: u64,
    id: Option<&str>,
    name: Option<&str>,
    arguments: Option<&str>,
    finish_reason: Option<&str>,
) -> Value {
    let mut function = Map::new();
    if let Some(name) = name {
        function.insert("name".to_string(), json!(name));
    }
    if let Some(arguments) = arguments {
        function.insert("arguments".to_string(), json!(arguments));
    }

    let mut call = Map::new();
    call.insert("index".to_string(), json!(index));
    if let Some(id) = id {
        call.insert("id".to_string(), json!(id));
    }
    call.insert("function".to_string(), Value::Object(function));

    let mut delta = Map::new();
    delta.insert(tool_calls_key(), Value::Array(vec![Value::Object(call)]));

    let mut choice = Map::new();
    choice.insert("delta".to_string(), Value::Object(delta));
    if let Some(finish_reason) = finish_reason {
        choice.insert("finish_reason".to_string(), json!(finish_reason));
    }

    json!({"choices":[Value::Object(choice)]})
}
