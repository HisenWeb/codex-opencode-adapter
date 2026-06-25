mod common;

use common::mock_upstream::start_mock_upstream;
use common::{adapter_url, start_adapter};
use serde_json::json;

#[tokio::test]
async fn test_e2e_request_payload_shape() {
    let (upstream_addr, _mock, received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let _ = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "instructions": "You are a helpful assistant.",
            "input": [{"type": "message", "role": "user", "content": "Hi"}],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    // Give a moment for the mock to record the payload.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let payloads = received.lock().await;
    assert_eq!(
        payloads.len(),
        1,
        "mock should have received exactly one payload"
    );

    let payload = &payloads[0];

    // Model prefix should be stripped.
    assert_eq!(
        payload["model"].as_str().unwrap(),
        "deepseek-v4-flash",
        "model prefix should be stripped"
    );

    // Should have messages array.
    let messages = payload["messages"].as_array().unwrap();
    assert!(!messages.is_empty(), "messages should not be empty");

    // First message should include the system instruction.
    let first_content = messages[0]["content"].as_str().unwrap();
    assert!(
        first_content.contains("You are a helpful assistant"),
        "system instruction should be in messages"
    );

    // stream should be false (adapter forces it).
    assert_eq!(
        payload["stream"].as_bool().unwrap(),
        false,
        "stream should be false for non-streaming"
    );
}

#[tokio::test]
async fn test_e2e_request_payload_streaming_shape() {
    let (upstream_addr, _mock, received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    let _ = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hi",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let payloads = received.lock().await;
    assert_eq!(payloads.len(), 1);

    let payload = &payloads[0];

    // stream should be true.
    assert_eq!(payload["stream"].as_bool().unwrap(), true);

    // stream_options should include_usage.
    assert_eq!(
        payload["stream_options"]["include_usage"]
            .as_bool()
            .unwrap(),
        true,
        "stream_options.include_usage should be true"
    );
}

#[tokio::test]
async fn test_e2e_auth_required() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, Some("my-secret-token".to_string())).await;
    let client = reqwest::Client::new();

    // Without auth → 401.
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
    assert_eq!(resp.status(), 401, "should return 401 without auth");

    // With auth → 200.
    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .header("Authorization", "Bearer my-secret-token")
        .json(&json!({
            "model": "opencode-go/deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "should return 200 with correct auth");
}

#[tokio::test]
async fn test_e2e_missing_model_prefix() {
    let (upstream_addr, _mock, _received) = start_mock_upstream().await;
    let adapter_addr = start_adapter(upstream_addr, None).await;
    let client = reqwest::Client::new();

    // Model without opencode-go/ prefix should be rejected.
    let resp = client
        .post(adapter_url(adapter_addr, "/v1/responses"))
        .json(&json!({
            "model": "deepseek-v4-flash",
            "input": "Hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "model without prefix should return 400");
}
