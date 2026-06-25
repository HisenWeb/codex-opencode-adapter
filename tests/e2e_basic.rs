mod common;

use common::mock_upstream::start_mock_upstream;
use common::{adapter_url, parse_sse_events, start_adapter};
use serde_json::{json, Value};

#[tokio::test]
async fn test_e2e_nonstreaming_text() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "non-streaming should return 200");
    let body: Value = resp.json().await.unwrap();

    // Verify Responses API structure.
    assert!(body.get("output").is_some(), "response must have output");
    let output = body["output"].as_array().unwrap();
    assert!(!output.is_empty(), "output must not be empty");

    // Should have a message item.
    let message_items: Vec<_> = output.iter().filter(|o| o["type"] == "message").collect();
    assert_eq!(
        message_items.len(),
        1,
        "should have exactly one message item"
    );

    // Content should contain mock response.
    let content_text: String = message_items[0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect();
    assert!(
        content_text.contains("Hello from mock!"),
        "content should contain mock response text"
    );

    // Model field should be present.
    assert!(
        body.get("model").is_some(),
        "response must have model field"
    );

    // Usage should be present.
    assert!(body.get("usage").is_some(), "response must have usage");
}

#[tokio::test]
async fn test_e2e_streaming_text() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "streaming should return 200");

    let body_text = resp.text().await.unwrap();
    let events = parse_sse_events(&body_text);

    // Should have output_text.delta events.
    let text_deltas: Vec<_> = events
        .iter()
        .filter(|(name, _)| name == "response.output_text.delta")
        .collect();
    assert!(
        !text_deltas.is_empty(),
        "should have output_text.delta events"
    );

    // Should have a response.completed terminal event.
    let completed = events.iter().find(|(name, _)| name == "response.completed");
    assert!(completed.is_some(), "should have response.completed event");

    // Final completed event should have output.
    let completed_data = &completed.unwrap().1;
    assert!(
        completed_data.get("response").is_some(),
        "completed event should have response"
    );
}
