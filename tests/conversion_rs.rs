use codex_opencode_adapter::conversion::responses_to_chat::build_chat_payload;
use codex_opencode_adapter::conversion::tool_context::ToolContext;
use serde_json::json;

#[test]
fn rust_tool_context_handles_namespace_and_custom() {
    let tools = json!([
        {
            "type": "namespace",
            "name": "mcp",
            "tools": [
                {
                    "type": "function",
                    "name": "read.file",
                    "description": "Read file",
                    "parameters": {"type":"object","properties":{}}
                }
            ]
        },
        {"type":"custom","name":"shell.exec"}
    ]);
    let context = ToolContext::build(Some(&tools));
    let names = context
        .chat_tools
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["mcp__read_file", "shell_exec"]);
    assert_eq!(context.restore_name("mcp__read_file"), "mcp__read.file");
    assert!(context.is_custom_tool_chat_name("shell_exec"));
}

#[test]
fn rust_request_transform_maps_tools_and_tool_choice() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-pro",
        "instructions": "System.",
        "input": [{"type":"message","role":"developer","content":"Dev."}, "Hi"],
        "tools": [{"type":"function","name":"mcp.read","parameters":{"type":"object"}}],
        "tool_choice": {"type":"function","name":"mcp.read"},
        "stream": true
    });
    let (payload, messages, reverse, _tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-pro", None, json!({})).unwrap();
    assert_eq!(
        messages[0],
        json!({"role":"system","content":"System.\n\nDev."})
    );
    assert_eq!(payload["stream_options"], json!({"include_usage": true}));
    assert_eq!(
        payload["tool_choice"],
        json!({"type":"function","function":{"name":"mcp_read"}})
    );
    assert_eq!(reverse.get("mcp_read").unwrap(), "mcp.read");
}

// ── Multimodal input tests ──

#[test]
fn rust_input_image_with_url_string() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-flash",
        "input": [
            {"type": "input_image", "url": "https://example.com/cat.png"},
            {"type": "message", "role": "user", "content": "What is this?"}
        ]
    });
    let (_payload, messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-flash", None, json!({})).unwrap();
    assert_eq!(messages[0]["role"], "user");
    let content = messages[0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "image_url");
    assert_eq!(
        content[0]["image_url"]["url"],
        "https://example.com/cat.png"
    );
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[1]["content"], "What is this?");
}

#[test]
fn rust_input_image_with_image_url_object() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-flash",
        "input": [
            {
                "type": "input_image",
                "image_url": {"url": "https://example.com/img.png", "detail": "high"}
            }
        ]
    });
    let (_payload, messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-flash", None, json!({})).unwrap();
    let content = messages[0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "image_url");
    assert_eq!(
        content[0]["image_url"]["url"],
        "https://example.com/img.png"
    );
    assert_eq!(content[0]["image_url"]["detail"], "high");
}

#[test]
fn rust_input_file() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-flash",
        "input": [
            {
                "type": "input_file",
                "file": {"filename": "doc.pdf", "file_data": "base64data"}
            }
        ]
    });
    let (_payload, messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-flash", None, json!({})).unwrap();
    let content = messages[0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "file");
    assert_eq!(content[0]["file"]["filename"], "doc.pdf");
    assert_eq!(content[0]["file"]["file_data"], "base64data");
}

#[test]
fn rust_input_file_fallback_to_object() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-flash",
        "input": [
            {"type": "input_file", "filename": "report.txt", "content": "text"}
        ]
    });
    let (_payload, messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-flash", None, json!({})).unwrap();
    let content = messages[0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "file");
    assert_eq!(content[0]["file"]["filename"], "report.txt");
}

#[test]
fn rust_input_audio() {
    let body = json!({
        "model": "opencode-go/deepseek-v4-flash",
        "input": [
            {
                "type": "input_audio",
                "input_audio": {"data": "base64audio", "format": "wav"}
            }
        ]
    });
    let (_payload, messages, _reverse, _tool_ctx) =
        build_chat_payload(&body, "deepseek-v4-flash", None, json!({})).unwrap();
    let content = messages[0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "input_audio");
    assert_eq!(content[0]["input_audio"]["data"], "base64audio");
    assert_eq!(content[0]["input_audio"]["format"], "wav");
}
