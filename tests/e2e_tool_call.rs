mod common;

use common::mock_upstream::start_mock_upstream_tool_call;
use common::{adapter_url, parse_sse_events, start_adapter};
use serde_json::{json, Value};

#[tokio::test]
async fn test_e2e_nonstreaming_tool_call() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_tool_call().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "What's the weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather for a city",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let output = body["output"].as_array().unwrap();

    // Should have a function_call item.
    let tool_items: Vec<_> = output
        .iter()
        .filter(|o| o["type"] == "function_call")
        .collect();
    assert_eq!(
        tool_items.len(),
        1,
        "should have exactly one function_call item"
    );

    let tool = &tool_items[0];
    assert_eq!(
        tool["name"].as_str().unwrap(),
        "get_weather",
        "tool name should match"
    );
    assert_eq!(
        tool["call_id"].as_str().unwrap(),
        "call_abc",
        "call_id should match"
    );

    let args: Value = serde_json::from_str(tool["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(
        args["city"].as_str().unwrap(),
        "Tokyo",
        "arguments should contain city"
    );
}

#[tokio::test]
async fn test_e2e_streaming_tool_call() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_tool_call().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "What's the weather?",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "Get weather for a city",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }],
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body_text = resp.text().await.unwrap();
    let events = parse_sse_events(&body_text);

    // Should have output_item.added for the function_call.
    let added_events: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.output_item.added")
        .collect();
    assert!(
        !added_events.is_empty(),
        "should have output_item.added events"
    );

    // Should have function_call_arguments.delta events.
    let arg_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.function_call_arguments.delta")
        .collect();
    assert!(
        !arg_deltas.is_empty(),
        "should have function_call_arguments.delta events"
    );

    // Should have function_call_arguments.done with complete args.
    let arg_done: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.function_call_arguments.done")
        .collect();
    assert_eq!(
        arg_done.len(),
        1,
        "should have exactly one function_call_arguments.done"
    );
    let done_args = &arg_done[0].1;
    assert!(
        done_args["arguments"].as_str().unwrap().contains("Tokyo"),
        "done arguments should contain Tokyo"
    );

    // Should have response.completed.
    assert!(
        events.iter().any(|(name, _)| name == "response.completed"),
        "should have response.completed event"
    );
}
