use codex_opencode_adapter::conversion::responses_to_chat::build_chat_payload;
use codex_opencode_adapter::conversion::{build_response, multimodal_input};
use codex_opencode_adapter::media_guard::{
    detect_multimodal_usage, find_unsupported_multimodal_input, is_multimodal_unsupported_error,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[test]
fn responses_request_maps_mixed_multimodal_content_parts() {
    let body = json!({
        "model": "opencode-go/multimodal-model",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                {"type":"input_text","text":"inspect these"},
                {"type":"input_image","image_url":"data:image/png;base64,abc"},
                {"type":"input_file","filename":"doc.pdf","file_data":"file-bytes"},
                {"type":"input_audio","input_audio":{"data":"audio-bytes","format":"wav"}}
            ]
        }]
    });

    let (payload, messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "multimodal-model", None, json!({})).unwrap();
    let content = messages[0]["content"].as_array().unwrap();
    assert_eq!(content[0], json!({"type":"text","text":"inspect these"}));
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,abc");
    assert_eq!(content[2]["type"], "file");
    assert_eq!(content[2]["file"]["filename"], "doc.pdf");
    assert_eq!(content[3]["type"], "input_audio");
    assert_eq!(payload["messages"][0]["content"], messages[0]["content"]);
}

#[test]
fn responses_request_maps_base64_source_image_to_image_url() {
    let part = json!({
        "type": "image",
        "source": {"type":"base64","media_type":"image/png","data":"abc123"}
    });
    let out = multimodal_input::image_to_chat_content_part(&part).unwrap();
    assert_eq!(out["type"], "image_url");
    assert_eq!(out["image_url"]["url"], "data:image/png;base64,abc123");
}

#[test]
fn responses_request_does_not_emit_chat_file_for_url_only_input_file() {
    let body = json!({
        "model": "opencode-go/multimodal-model",
        "input": [
            {"type":"input_file","file":{"url":"https://example.com/doc.pdf","filename":"doc.pdf"}},
            "fallback text"
        ]
    });
    let (_payload, messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "multimodal-model", None, json!({})).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["content"], "fallback text");
}

#[test]
fn media_guard_rejects_known_text_only_models_before_upstream() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "input": [{"type":"input_image","image_url":"data:image/png;base64,abc"}]
    });
    let (payload, _messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-pro", None, json!({})).unwrap();

    let usage = detect_multimodal_usage(&payload);
    assert!(usage.image);
    assert!(find_unsupported_multimodal_input("deepseek-v4-pro", &payload).is_some());
}

#[test]
fn media_guard_passes_unknown_model_through() {
    let body = json!({
        "model": "opencode-go/unknown-model",
        "input": [{"type":"input_image","image_url":"data:image/png;base64,abc"}]
    });
    let (payload, _messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "unknown-model", None, json!({})).unwrap();
    assert!(find_unsupported_multimodal_input("unknown-model", &payload).is_none());
}

#[test]
fn detects_upstream_multimodal_unsupported_errors() {
    assert!(is_multimodal_unsupported_error(
        "Failed to deserialize the JSON body into the target type: messages[11]: unknown variant image_url, expected text"
    ));
    assert!(is_multimodal_unsupported_error(
        "selected model does not support image input"
    ));
    assert!(!is_multimodal_unsupported_error("quota exceeded"));
}

#[test]
fn chat_response_content_array_extracts_text_and_keeps_json_fallback() {
    let body = json!({"model":"opencode-go/test","input":"hi"});
    let (payload, messages, _reverse, tool_ctx) =
        build_chat_payload(&body, "test", None, json!({})).unwrap();
    let stored = Arc::new(Mutex::new(None));
    let stored_clone = stored.clone();
    let chat = json!({
        "choices": [{
            "message": {
                "role":"assistant",
                "content": [
                    {"type":"text","text":"hello"},
                    {"type":"image_url","image_url":{"url":"https://example.com/out.png"}}
                ]
            },
            "finish_reason":"stop"
        }],
        "usage": {"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}
    });

    let response = build_response(
        &body,
        &chat,
        "opencode-go/test",
        payload["model"].as_str().unwrap(),
        &messages,
        &tool_ctx,
        |item| {
            *stored_clone.lock().unwrap() = Some(item);
            Ok(())
        },
    )
    .unwrap();

    let text = response["output"][0]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("hello"));
    assert!(text.contains("image_url"));
}
