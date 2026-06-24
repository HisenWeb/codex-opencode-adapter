use codex_opencode_adapter::conversion::responses_to_chat::build_chat_payload;
use codex_opencode_adapter::conversion::StreamAssembler;
use codex_opencode_adapter::state::StoredResponse;
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};

#[test]
fn streaming_function_tool_accepts_split_id_name_and_arguments() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Run a command",
        "tools": [{"type":"function","name":"run","parameters":{"type":"object"}}],
        "stream": true
    });
    let (mut assembler, events, stored) = new_assembler(body);

    assembler.start().unwrap();
    assembler
        .accept(&stream_tool_chunk(0, Some("call_run"), None, None, None))
        .unwrap();
    assembler
        .accept(&stream_tool_chunk(0, None, Some("run"), Some("{\"cmd\":"), None))
        .unwrap();
    assembler
        .accept(&stream_tool_chunk(
            0,
            None,
            None,
            Some("\"echo ok\"}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();

    let output = response["output"].as_array().unwrap();
    let call = output
        .iter()
        .find(|item| item["type"] == "function_call")
        .unwrap();
    assert_eq!(call["call_id"], "call_run");
    assert_eq!(call["name"], "run");
    assert_eq!(call["arguments"], "{\"cmd\":\"echo ok\"}");

    let events = events.lock().unwrap();
    let arg_deltas = events
        .iter()
        .filter(|(name, _)| name == "response.function_call_arguments.delta")
        .collect::<Vec<_>>();
    assert_eq!(arg_deltas.len(), 2);
    let done = events
        .iter()
        .find(|(name, _)| name == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(done.1["arguments"], "{\"cmd\":\"echo ok\"}");

    let stored = stored.lock().unwrap();
    assert_eq!(stored[0].pending_call_ids, vec!["call_run".to_string()]);
}

#[test]
fn streaming_function_tool_buffers_arguments_until_id_and_name_arrive() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Run a command",
        "tools": [{"type":"function","name":"run","parameters":{"type":"object"}}],
        "stream": true
    });
    let (mut assembler, events, _stored) = new_assembler(body);

    assembler.start().unwrap();
    assembler
        .accept(&stream_tool_chunk(0, None, None, Some("{\"cmd\":"), None))
        .unwrap();
    {
        let events = events.lock().unwrap();
        assert!(!events
            .iter()
            .any(|(name, _)| name == "response.output_item.added"));
        assert!(!events
            .iter()
            .any(|(name, _)| name == "response.function_call_arguments.delta"));
    }

    assembler
        .accept(&stream_tool_chunk(
            0,
            Some("call_run"),
            Some("run"),
            Some("\"echo ok\"}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();

    let output = response["output"].as_array().unwrap();
    let call = output
        .iter()
        .find(|item| item["type"] == "function_call")
        .unwrap();
    assert_eq!(call["arguments"], "{\"cmd\":\"echo ok\"}");

    let events = events.lock().unwrap();
    let arg_deltas = events
        .iter()
        .filter(|(name, _)| name == "response.function_call_arguments.delta")
        .collect::<Vec<_>>();
    assert_eq!(arg_deltas.len(), 1);
    assert_eq!(arg_deltas[0].1["delta"], "{\"cmd\":\"echo ok\"}");
}

#[test]
fn streaming_multiple_tool_call_indexes_are_kept_separate() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Use two tools",
        "tools": [
            {"type":"function","name":"run","parameters":{"type":"object"}},
            {"type":"function","name":"list","parameters":{"type":"object"}}
        ],
        "stream": true
    });
    let (mut assembler, _events, stored) = new_assembler(body);

    assembler.start().unwrap();
    assembler
        .accept(&stream_tool_chunk(
            1,
            Some("call_list"),
            Some("list"),
            Some("{\"path\":\"src\"}"),
            None,
        ))
        .unwrap();
    assembler
        .accept(&stream_tool_chunk(
            0,
            Some("call_run"),
            Some("run"),
            Some("{\"cmd\":\"cargo test\"}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();

    let output = response["output"].as_array().unwrap();
    let run = output
        .iter()
        .find(|item| item["call_id"] == "call_run")
        .unwrap();
    let list = output
        .iter()
        .find(|item| item["call_id"] == "call_list")
        .unwrap();
    assert_eq!(run["name"], "run");
    assert_eq!(run["arguments"], "{\"cmd\":\"cargo test\"}");
    assert_eq!(list["name"], "list");
    assert_eq!(list["arguments"], "{\"path\":\"src\"}");

    let stored = stored.lock().unwrap();
    assert_eq!(
        stored[0].pending_call_ids,
        vec!["call_run".to_string(), "call_list".to_string()]
    );
}

#[test]
fn streaming_function_custom_and_tool_search_types_do_not_cross() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Use all tool kinds",
        "tools": [
            {"type":"function","name":"run","parameters":{"type":"object"}},
            {"type":"custom","name":"custom.echo"},
            {"type":"tool_search"}
        ],
        "stream": true
    });
    let (mut assembler, events, _stored) = new_assembler(body);

    assembler.start().unwrap();
    assembler
        .accept(&stream_tool_chunk(
            0,
            Some("call_run"),
            Some("run"),
            Some("{\"cmd\":\"echo ok\"}"),
            None,
        ))
        .unwrap();
    assembler
        .accept(&stream_tool_chunk(
            1,
            Some("call_custom"),
            Some("custom_echo"),
            Some("{\"input\":\"hello\"}"),
            None,
        ))
        .unwrap();
    assembler
        .accept(&stream_tool_chunk(
            2,
            Some("call_search"),
            Some("tool_search"),
            Some("{\"query\":\"files\",\"limit\":3}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();

    let output = response["output"].as_array().unwrap();
    let function = output
        .iter()
        .find(|item| item["call_id"] == "call_run")
        .unwrap();
    let custom = output
        .iter()
        .find(|item| item["call_id"] == "call_custom")
        .unwrap();
    let search = output
        .iter()
        .find(|item| item["call_id"] == "call_search")
        .unwrap();
    assert_eq!(function["type"], "function_call");
    assert_eq!(custom["type"], "custom_tool_call");
    assert_eq!(custom["input"], "hello");
    assert_eq!(search["type"], "tool_search_call");
    assert_eq!(search["arguments"]["query"], "files");
    assert_eq!(search["arguments"]["limit"], 3);

    let events = events.lock().unwrap();
    let custom_dones = events
        .iter()
        .filter(|(name, _)| name == "response.custom_tool_call_input.done")
        .collect::<Vec<_>>();
    assert_eq!(custom_dones.len(), 1);
    assert_eq!(custom_dones[0].1["item_id"], custom["id"]);

    let function_dones = events
        .iter()
        .filter(|(name, _)| name == "response.function_call_arguments.done")
        .collect::<Vec<_>>();
    assert_eq!(function_dones.len(), 2);
    assert!(function_dones
        .iter()
        .any(|(_, data)| data["call_id"] == "call_run"));
    assert!(function_dones
        .iter()
        .any(|(_, data)| data["call_id"] == "call_search"));
}

#[test]
fn streaming_tool_call_without_name_is_skipped_and_does_not_pollute_state() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": "Malformed tool call",
        "tools": [{"type":"function","name":"run","parameters":{"type":"object"}}],
        "stream": true
    });
    let (mut assembler, events, stored) = new_assembler(body);

    assembler.start().unwrap();
    assembler
        .accept(&stream_tool_chunk(
            0,
            Some("call_missing_name"),
            None,
            Some("{\"cmd\":\"echo ok\"}"),
            Some("tool_calls"),
        ))
        .unwrap();
    let response = assembler.finalize().unwrap();

    assert!(response["output"].as_array().unwrap().is_empty());
    let stored = stored.lock().unwrap();
    assert!(stored[0].pending_call_ids.is_empty());
    assert!(stored[0].output.is_empty());

    let events = events.lock().unwrap();
    assert!(!events.iter().any(|(name, data)| {
        name == "response.output_item.added" && data["item"]["type"].as_str() == Some("function_call")
    }));
}

fn new_assembler(
    body: Value,
) -> (
    StreamAssembler,
    Arc<Mutex<Vec<(String, Value)>>>,
    Arc<Mutex<Vec<StoredResponse>>>,
) {
    let (_payload, messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-pro", None, json!({})).unwrap();
    let events = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let stored = Arc::new(Mutex::new(Vec::<StoredResponse>::new()));
    let events_for_emit = Arc::clone(&events);
    let stored_for_put = Arc::clone(&stored);
    let assembler = StreamAssembler::new(
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
    (assembler, events, stored)
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
    delta.insert("tool_calls".to_string(), Value::Array(vec![Value::Object(call)]));

    let mut choice = Map::new();
    choice.insert("delta".to_string(), Value::Object(delta));
    if let Some(finish_reason) = finish_reason {
        choice.insert("finish_reason".to_string(), json!(finish_reason));
    }

    json!({"choices":[Value::Object(choice)]})
}
