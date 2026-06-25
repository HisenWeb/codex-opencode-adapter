mod common;

use common::{
    extract_output_text, find_output_item, parse_sse_events, real_multimodal_smoke_input,
    start_real_adapter, RealSmokeConfig,
};
use serde_json::{json, Value};

#[tokio::test]
#[ignore] // Run explicitly: `cargo test --test e2e_real_smoke test_e2e_real_validation_suite -- --ignored --nocapture`
async fn test_e2e_real_validation_suite() {
    let Some(config) = RealSmokeConfig::from_env() else {
        return;
    };

    let addr = start_real_adapter(&config).await;
    let http_client = reqwest::Client::new();

    let models_resp = http_client
        .get(format!("http://{addr}/v1/models"))
        .send()
        .await
        .expect("models request should succeed");
    assert_eq!(models_resp.status(), 200, "models smoke should return 200");
    let models_body: Value = models_resp.json().await.unwrap();
    let empty_models = Vec::new();
    let model_ids = models_body["data"]
        .as_array()
        .unwrap_or(&empty_models)
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect::<Vec<_>>();
    assert!(
        model_ids.iter().any(|id| *id == config.text_model),
        "text model should appear in /v1/models"
    );
    assert!(
        model_ids.iter().any(|id| *id == config.vision_model),
        "vision model should appear in /v1/models"
    );

    let text_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "input": "Reply with exactly adapter-ok.",
            "stream": false
        }))
        .send()
        .await
        .expect("real text smoke request should succeed");
    assert_eq!(text_resp.status(), 200, "real text smoke should return 200");
    let text_body: Value = text_resp.json().await.unwrap();
    assert_eq!(text_body["status"], "completed");
    let text_output = extract_output_text(&text_body);
    assert!(
        text_output.to_lowercase().contains("adapter-ok"),
        "text smoke should contain adapter-ok, got: {text_output}"
    );

    let stream_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "input": "Reply with exactly stream-ok.",
            "stream": true
        }))
        .send()
        .await
        .expect("real stream smoke request should succeed");
    assert_eq!(
        stream_resp.status(),
        200,
        "real stream smoke should return 200"
    );
    let stream_body = stream_resp.text().await.unwrap();
    let stream_events = parse_sse_events(&stream_body);
    assert!(
        stream_events
            .iter()
            .any(|(name, _)| name == "response.completed"),
        "real stream smoke should emit response.completed"
    );

    let tool_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "input": "Call the tool once with cmd set to echo tool-ok.",
            "tools": [{
                "type": "function",
                "name": "run",
                "description": "Run a shell command",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "cmd": {"type": "string"}
                    },
                    "required": ["cmd"]
                }
            }],
            "stream": false
        }))
        .send()
        .await
        .expect("real tool-call smoke request should succeed");
    assert_eq!(tool_resp.status(), 200, "real tool smoke should return 200");
    let tool_body: Value = tool_resp.json().await.unwrap();
    assert_eq!(tool_body["status"], "completed");
    let tool_call = find_output_item(&tool_body, "function_call")
        .cloned()
        .expect("real tool smoke should produce a function_call item");
    let tool_call_id = tool_call["call_id"]
        .as_str()
        .expect("tool call should have call_id")
        .to_string();
    let tool_response_id = tool_body["id"]
        .as_str()
        .expect("tool smoke should produce response id")
        .to_string();

    let continuation_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "previous_response_id": tool_response_id,
            "input": [{
                "type": "function_call_output",
                "call_id": tool_call_id,
                "output": "tool-ok"
            }],
            "stream": false
        }))
        .send()
        .await
        .expect("real continuation smoke request should succeed");
    assert_eq!(
        continuation_resp.status(),
        200,
        "real continuation smoke should return 200"
    );
    let continuation_body: Value = continuation_resp.json().await.unwrap();
    assert_eq!(continuation_body["status"], "completed");
    assert!(
        extract_output_text(&continuation_body)
            .to_lowercase()
            .contains("tool-ok"),
        "continuation smoke should mention tool-ok"
    );

    let streamed_tool_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "input": "Call the tool once with cmd set to echo stream-tool-ok.",
            "tools": [{
                "type": "function",
                "name": "run",
                "description": "Run a shell command",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "cmd": {"type": "string"}
                    },
                    "required": ["cmd"]
                }
            }],
            "stream": true
        }))
        .send()
        .await
        .expect("real streamed tool-call smoke request should succeed");
    assert_eq!(
        streamed_tool_resp.status(),
        200,
        "real streamed tool smoke should return 200"
    );
    let streamed_tool_body = streamed_tool_resp.text().await.unwrap();
    let streamed_tool_events = parse_sse_events(&streamed_tool_body);
    assert!(
        streamed_tool_events
            .iter()
            .any(|(name, _)| name == "response.output_item.added"),
        "real streamed tool smoke should emit response.output_item.added"
    );
    assert!(
        streamed_tool_events
            .iter()
            .any(|(name, _)| name == "response.function_call_arguments.delta"),
        "real streamed tool smoke should emit response.function_call_arguments.delta"
    );
    let streamed_tool_done = streamed_tool_events
        .iter()
        .find(|(name, _)| name == "response.function_call_arguments.done")
        .expect("real streamed tool smoke should emit response.function_call_arguments.done");
    assert!(
        streamed_tool_done.1["arguments"]
            .as_str()
            .unwrap_or("")
            .contains("stream-tool-ok"),
        "real streamed tool smoke should contain stream-tool-ok"
    );
    assert!(
        streamed_tool_events
            .iter()
            .any(|(name, _)| name == "response.completed"),
        "real streamed tool smoke should emit response.completed"
    );

    let custom_tool_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "input": "Call the custom tool `shell` once with raw input `echo custom-ok`.",
            "tools": [{
                "type": "custom",
                "name": "shell",
                "description": "Run a shell command from a raw string input."
            }],
            "stream": false
        }))
        .send()
        .await
        .expect("real custom-tool smoke request should succeed");
    assert_eq!(
        custom_tool_resp.status(),
        200,
        "real custom tool smoke should return 200"
    );
    let custom_tool_body: Value = custom_tool_resp.json().await.unwrap();
    assert_eq!(custom_tool_body["status"], "completed");
    let custom_tool_call = find_output_item(&custom_tool_body, "custom_tool_call")
        .expect("custom tool smoke should produce custom_tool_call");
    assert_eq!(custom_tool_call["name"], "shell");
    let custom_tool_input = custom_tool_call["input"].as_str().unwrap_or("").to_string();
    assert!(
        !custom_tool_input.trim().is_empty(),
        "custom tool smoke should preserve raw input"
    );
    let custom_tool_call_id = custom_tool_call["call_id"]
        .as_str()
        .expect("custom tool smoke should include call_id")
        .to_string();
    let custom_tool_response_id = custom_tool_body["id"]
        .as_str()
        .expect("custom tool smoke should produce response id")
        .to_string();
    let custom_tool_continuation_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "previous_response_id": custom_tool_response_id,
            "input": [{
                "type": "custom_tool_call_output",
                "call_id": custom_tool_call_id,
                "output": "custom-ok"
            }],
            "stream": false
        }))
        .send()
        .await
        .expect("real custom-tool continuation should succeed");
    assert_eq!(
        custom_tool_continuation_resp.status(),
        200,
        "real custom-tool continuation should return 200"
    );
    let custom_tool_continuation_body: Value = custom_tool_continuation_resp.json().await.unwrap();
    assert_eq!(custom_tool_continuation_body["status"], "completed");
    assert!(
        !extract_output_text(&custom_tool_continuation_body)
            .trim()
            .is_empty(),
        "custom-tool continuation should produce output"
    );

    let tool_search_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "input": "Use tool_search once to search for filesystem-related tools.",
            "tools": [{
                "type": "tool_search"
            }],
            "stream": false
        }))
        .send()
        .await
        .expect("real tool-search smoke request should succeed");
    assert_eq!(
        tool_search_resp.status(),
        200,
        "real tool-search smoke should return 200"
    );
    let tool_search_body: Value = tool_search_resp.json().await.unwrap();
    assert_eq!(tool_search_body["status"], "completed");
    let tool_search_call = find_output_item(&tool_search_body, "tool_search_call")
        .expect("tool-search smoke should produce tool_search_call");
    assert_eq!(tool_search_call["execution"], "client");
    assert!(
        tool_search_call["arguments"].is_object(),
        "tool-search arguments should keep JSON object shape"
    );
    let tool_search_call_id = tool_search_call["call_id"]
        .as_str()
        .expect("tool-search smoke should include call_id")
        .to_string();
    let tool_search_response_id = tool_search_body["id"]
        .as_str()
        .expect("tool-search smoke should produce response id")
        .to_string();
    let tool_search_continuation_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "previous_response_id": tool_search_response_id,
            "input": [{
                "type": "tool_search_output",
                "call_id": tool_search_call_id,
                "output": "search-ok",
                "tools": [{
                    "type": "function",
                    "name": "search_result_tool",
                    "description": "A tool discovered by tool search.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"}
                        }
                    }
                }]
            }],
            "stream": false
        }))
        .send()
        .await
        .expect("real tool-search continuation should succeed");
    assert_eq!(
        tool_search_continuation_resp.status(),
        200,
        "real tool-search continuation should return 200"
    );
    let tool_search_continuation_body: Value = tool_search_continuation_resp.json().await.unwrap();
    assert_eq!(tool_search_continuation_body["status"], "completed");
    assert!(
        tool_search_continuation_body["output"]
            .as_array()
            .map(|items| !items.is_empty())
            .unwrap_or(false),
        "tool-search continuation should produce a non-empty output array"
    );

    let text_model_multimodal_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.text_model,
            "input": real_multimodal_smoke_input(),
            "stream": false
        }))
        .send()
        .await
        .expect("text-model multimodal rejection request should succeed");
    let text_model_multimodal_status = text_model_multimodal_resp.status();
    let text_model_multimodal_body: Value = text_model_multimodal_resp.json().await.unwrap();
    assert_eq!(text_model_multimodal_status, 200);
    assert_eq!(text_model_multimodal_body["status"], "failed");
    assert_eq!(
        text_model_multimodal_body["error"]["code"],
        "unsupported_multimodal_input"
    );

    let vision_resp = http_client
        .post(format!("http://{addr}/v1/responses"))
        .json(&json!({
            "model": config.vision_model,
            "input": real_multimodal_smoke_input(),
            "stream": false
        }))
        .send()
        .await
        .expect("real multimodal smoke request should succeed");
    let vision_status = vision_resp.status();
    let vision_body: Value = vision_resp.json().await.unwrap();
    assert_eq!(vision_status, 200, "vision smoke should return 200");
    assert_eq!(vision_body["status"], "completed");
    let vision_output = extract_output_text(&vision_body);
    assert!(
        !vision_output.trim().is_empty(),
        "vision smoke should produce visible output"
    );

    eprintln!(
        "Real validation suite passed with text_model={} vision_model={}",
        config.text_model, config.vision_model
    );
}
