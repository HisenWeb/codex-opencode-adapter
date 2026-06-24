use codex_opencode_adapter::conversion::responses_to_chat::build_chat_payload;
use codex_opencode_adapter::conversion::StreamAssembler;
use codex_opencode_adapter::state::StoredResponse;
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};

#[test]
fn streaming_content_before_tool_call_does_not_emit_message_output() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Use a tool",
        "tools": [{"type":"function","name":"run","parameters":{"type":"object"}}],
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
        .accept(&stream_content_chunk("I should call a tool first.", None))
        .unwrap();

    {
        let events = events.lock().unwrap();
        assert!(!events
            .iter()
            .any(|(name, _)| name == "response.output_text.delta"));
        assert!(!events.iter().any(|(name, data)| {
            name == "response.output_item.added" && data["item"]["type"].as_str() == Some("message")
        }));
    }

    assembler
        .accept(&stream_tool_chunk(
            0,
            Some("call_run"),
            Some("run"),
            Some("{\"cmd\":\"echo ok\"}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();
    let events = events.lock().unwrap();

    assert!(!events
        .iter()
        .any(|(name, _)| name == "response.output_text.delta"));
    assert!(!events.iter().any(|(name, data)| {
        name == "response.output_item.done" && data["item"]["type"].as_str() == Some("message")
    }));
    assert!(events
        .iter()
        .any(|(name, _)| name == "response.function_call_arguments.done"));

    let output = response["output"].as_array().unwrap();
    assert!(output.iter().any(|item| item["type"] == "function_call"));
    assert!(!output.iter().any(|item| item["type"] == "message"));

    let stored = stored.lock().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].pending_call_ids, vec!["call_run".to_string()]);
    let assistant = stored[0].messages.last().unwrap();
    assert_eq!(assistant["content"], "I should call a tool first.");
    assert!(assistant["tool_calls"].as_array().is_some());
}

#[test]
fn streaming_content_without_tools_keeps_incremental_text_deltas() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Answer normally",
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
        .accept(&stream_content_chunk("hello ", None))
        .unwrap();
    {
        let events = events.lock().unwrap();
        let text_deltas = events
            .iter()
            .filter(|(name, _)| name == "response.output_text.delta")
            .collect::<Vec<_>>();
        assert_eq!(text_deltas.len(), 1);
        assert_eq!(text_deltas[0].1["delta"], "hello ");
    }

    assembler
        .accept(&stream_content_chunk("world", Some("stop")))
        .unwrap();
    let response = assembler.finalize().unwrap();
    let events = events.lock().unwrap();

    let text_deltas = events
        .iter()
        .filter(|(name, _)| name == "response.output_text.delta")
        .collect::<Vec<_>>();
    assert_eq!(text_deltas.len(), 2);
    assert_eq!(text_deltas[0].1["delta"], "hello ");
    assert_eq!(text_deltas[1].1["delta"], "world");

    let output = response["output"].as_array().unwrap();
    let message = output
        .iter()
        .find(|item| item["type"] == "message")
        .unwrap();
    assert_eq!(message["content"][0]["text"], "hello world");
}

fn stream_content_chunk(content: &str, finish_reason: Option<&str>) -> Value {
    let mut delta = Map::new();
    delta.insert("content".to_string(), json!(content));

    let mut choice = Map::new();
    choice.insert("delta".to_string(), Value::Object(delta));
    if let Some(finish_reason) = finish_reason {
        choice.insert("finish_reason".to_string(), json!(finish_reason));
    }

    json!({"choices":[Value::Object(choice)]})
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
    delta.insert(
        "tool_calls".to_string(),
        Value::Array(vec![Value::Object(call)]),
    );

    let mut choice = Map::new();
    choice.insert("delta".to_string(), Value::Object(delta));
    if let Some(finish_reason) = finish_reason {
        choice.insert("finish_reason".to_string(), json!(finish_reason));
    }

    json!({"choices":[Value::Object(choice)]})
}
