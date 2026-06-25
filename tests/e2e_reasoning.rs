mod common;

use common::mock_upstream::start_mock_upstream_reasoning;
use common::{adapter_url, parse_sse_events, start_adapter};
use serde_json::{json, Value};

#[tokio::test]
async fn test_e2e_nonstreaming_with_reasoning() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_reasoning().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Think about it",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let output = body["output"].as_array().unwrap();

    // Should have a reasoning item.
    let reasoning_items: Vec<_> = output.iter().filter(|o| o["type"] == "reasoning").collect();
    assert_eq!(
        reasoning_items.len(),
        1,
        "should have exactly one reasoning item"
    );

    // Reasoning content should contain expected text.
    // Note: reasoning_item uses "summary" field, not "content".
    let reasoning_summary: String = reasoning_items[0]["summary"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect();
    assert!(
        reasoning_summary.contains("Let me think..."),
        "reasoning summary should contain expected text"
    );

    // Should also have a message item after reasoning.
    let message_items: Vec<_> = output.iter().filter(|o| o["type"] == "message").collect();
    assert_eq!(
        message_items.len(),
        1,
        "should have exactly one message item"
    );
}

#[tokio::test]
async fn test_e2e_streaming_with_reasoning() {
    let (upstream_addr, _mock, _received) = start_mock_upstream_reasoning().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Think about it",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body_text = resp.text().await.unwrap();
    let events = parse_sse_events(&body_text);

    // Should have reasoning summary text delta events.
    let reasoning_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.reasoning_summary_text.delta")
        .collect();
    assert!(
        !reasoning_deltas.is_empty(),
        "should have reasoning_summary_text.delta events"
    );

    // Should have output_text delta events for the content.
    let text_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.output_text.delta")
        .collect();
    assert!(
        !text_deltas.is_empty(),
        "should have output_text.delta events"
    );

    // Should have response.completed.
    assert!(
        events.iter().any(|(name, _)| name == "response.completed"),
        "should have response.completed event"
    );
}
