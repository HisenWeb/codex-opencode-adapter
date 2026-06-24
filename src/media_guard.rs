use serde_json::Value;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MultimodalUsage {
    pub image: bool,
    pub file: bool,
    pub audio: bool,
}

impl MultimodalUsage {
    pub fn any(&self) -> bool {
        self.image || self.file || self.audio
    }

    pub fn labels(&self) -> Vec<&'static str> {
        let mut labels = Vec::new();
        if self.image {
            labels.push("image");
        }
        if self.file {
            labels.push("file");
        }
        if self.audio {
            labels.push("audio");
        }
        labels
    }
}

pub fn detect_multimodal_usage(value: &Value) -> MultimodalUsage {
    let mut usage = MultimodalUsage::default();
    scan_value(value, &mut usage);
    usage
}

pub fn find_unsupported_multimodal_input(model: &str, payload: &Value) -> Option<String> {
    let usage = detect_multimodal_usage(payload);
    if !usage.any() || !is_known_text_only_model(model) {
        return None;
    }
    Some(unsupported_multimodal_message(model, &usage))
}

pub fn is_known_text_only_model(model: &str) -> bool {
    let normalized = normalize_model(model);
    if normalized.is_empty() {
        return false;
    }
    if contains_any(
        &normalized,
        &["vision", "multimodal", "qwen-vl", "glm-v", "kimi-vl"],
    ) {
        return false;
    }
    matches!(
        normalized.as_str(),
        "deepseek-v4-pro" | "deepseek-v4-flash" | "deepseek-v3" | "deepseek-r1"
    )
}

pub fn is_multimodal_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("unknown variant image_url")
        || (normalized.contains("image_url") && normalized.contains("expected text"))
        || normalized.contains("does not support image")
        || normalized.contains("doesn't support image")
        || normalized.contains("image input is not supported")
        || normalized.contains("vision not supported")
        || (normalized.contains("does not support") && normalized.contains("vision"))
        || (normalized.contains("multimodal") && normalized.contains("not supported"))
}

pub fn unsupported_multimodal_error_message() -> &'static str {
    "The selected upstream model does not support multimodal input. Use a vision-capable model or remove image/file/audio content."
}

fn unsupported_multimodal_message(model: &str, usage: &MultimodalUsage) -> String {
    let labels = usage.labels().join("/");
    format!(
        "The selected upstream model `{}` does not support {} input. Use a multimodal model or remove the media content.",
        model, labels
    )
}

fn scan_value(value: &Value, usage: &mut MultimodalUsage) {
    match value {
        Value::Array(items) => {
            for item in items {
                scan_value(item, usage);
            }
        }
        Value::Object(map) => {
            if let Some(kind) = map.get("type").and_then(Value::as_str) {
                match kind {
                    "image" | "image_url" | "input_image" => usage.image = true,
                    "file" | "input_file" => usage.file = true,
                    "audio" | "input_audio" => usage.audio = true,
                    _ => {}
                }
            }
            for value in map.values() {
                scan_value(value, usage);
            }
        }
        _ => {}
    }
}

fn normalize_model(model: &str) -> String {
    model.trim()
        .trim_start_matches("opencode-go/")
        .to_ascii_lowercase()
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_multimodal_usage_in_chat_payload() {
        let payload = json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type":"text","text":"look"},
                    {"type":"image_url","image_url":{"url":"data:image/png;base64,abc"}},
                    {"type":"file","file":{"file_data":"abc"}},
                    {"type":"input_audio","input_audio":{"data":"abc","format":"wav"}}
                ]
            }]
        });
        let usage = detect_multimodal_usage(&payload);
        assert!(usage.image);
        assert!(usage.file);
        assert!(usage.audio);
    }

    #[test]
    fn known_text_only_model_rejects_media() {
        let payload = json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"x"}}]}]});
        let error = find_unsupported_multimodal_input("deepseek-v4-pro", &payload).unwrap();
        assert!(error.contains("deepseek-v4-pro"));
        assert!(error.contains("image"));
    }

    #[test]
    fn unknown_model_passes_through_media() {
        let payload = json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"x"}}]}]});
        assert!(find_unsupported_multimodal_input("experimental-model", &payload).is_none());
    }

    #[test]
    fn detects_upstream_image_url_deserialize_errors() {
        assert!(is_multimodal_unsupported_error(
            "Failed to deserialize the JSON body into the target type: messages[11]: unknown variant image_url, expected text"
        ));
        assert!(is_multimodal_unsupported_error(
            "this model does not support image input"
        ));
        assert!(!is_multimodal_unsupported_error("invalid api key"));
    }
}
