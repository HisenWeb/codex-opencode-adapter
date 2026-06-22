use serde_json::Value;

pub fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

pub fn as_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let text = as_text(item);
                (!text.is_empty()).then_some(text)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.to_string();
            }
            if let Some(refusal) = map.get("refusal").and_then(Value::as_str) {
                return refusal.to_string();
            }
            if let Some(content) = map.get("content") {
                return as_text(content);
            }
            if let Some(output) = map.get("output") {
                return as_text(output);
            }
            compact_json(value)
        }
    }
}

pub fn arguments_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(value) => compact_json(value),
        None => "{}".to_string(),
    }
}

pub fn reasoning_text(value: &Value) -> Option<String> {
    for key in ["reasoning_content", "reasoning", "thinking"] {
        if let Some(field) = value.get(key) {
            let text = as_text(field);
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

pub fn canonicalize_json_string_if_parseable(value: &str) -> String {
    match serde_json::from_str::<Value>(value) {
        Ok(parsed) => compact_json(&parsed),
        Err(_) => value.to_string(),
    }
}
