use serde_json::Value;

const THINK_OPEN_TAG: &str = concat!("<", "think", ">");
const THINK_CLOSE_TAG: &str = concat!("</", "think", ">");

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
    // 1. reasoning_content / thinking as plain string
    for key in ["reasoning_content", "thinking"] {
        if let Some(text) = value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return Some(text.to_string());
        }
    }
    // 2. reasoning: string OR object with content/text/summary
    if let Some(reasoning) = value.get("reasoning") {
        if let Some(text) = reasoning.as_str().filter(|s| !s.is_empty()) {
            return Some(text.to_string());
        }
        if let Some(obj) = reasoning.as_object() {
            for key in ["content", "text", "summary"] {
                if let Some(text) = obj
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    return Some(text.to_string());
                }
            }
        }
    }
    // 3. reasoning_details: string, array of parts, or single object
    if let Some(details) = value.get("reasoning_details") {
        if let Some(text) = extract_reasoning_details_text(details) {
            return Some(text);
        }
    }
    None
}

fn extract_reasoning_details_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.is_empty()).then(|| text.to_string()),
        Value::Array(parts) => {
            let text: String = parts
                .iter()
                .filter_map(|part| {
                    for key in ["text", "content", "summary"] {
                        if let Some(t) = part
                            .get(key)
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                        {
                            return Some(t.to_string());
                        }
                    }
                    // Handle nested parts
                    if let Some(nested) = part.get("parts").and_then(Value::as_array) {
                        let inner: String = nested
                            .iter()
                            .filter_map(extract_reasoning_detail_part_text)
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        if !inner.is_empty() {
                            return Some(inner);
                        }
                    }
                    None
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(_) => extract_reasoning_detail_part_text(value),
        _ => None,
    }
}

fn extract_reasoning_detail_part_text(value: &Value) -> Option<String> {
    for key in ["text", "content", "summary"] {
        if let Some(text) = value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return Some(text.to_string());
        }
    }
    if let Some(parts) = value.get("parts").and_then(Value::as_array) {
        let text: String = parts
            .iter()
            .filter_map(extract_reasoning_detail_part_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        return (!text.is_empty()).then_some(text);
    }
    None
}

pub fn canonicalize_json_string_if_parseable(value: &str) -> String {
    match serde_json::from_str::<Value>(value) {
        Ok(parsed) => compact_json(&parsed),
        Err(_) => value.to_string(),
    }
}

pub fn split_leading_think_block(text: &str) -> Option<(String, String)> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    if !after_ws.starts_with(THINK_OPEN_TAG) {
        return None;
    }

    let body_start = leading_ws_len + THINK_OPEN_TAG.len();
    let close_relative = text[body_start..].find(THINK_CLOSE_TAG)?;
    let close_start = body_start + close_relative;
    let answer_start = close_start + THINK_CLOSE_TAG.len();

    Some((
        text[body_start..close_start].trim().to_string(),
        strip_think_answer_separator(&text[answer_start..]).to_string(),
    ))
}

pub fn strip_leading_think_open_tag(text: &str) -> Option<String> {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    after_ws
        .strip_prefix(THINK_OPEN_TAG)
        .map(|value| value.trim().to_string())
}

pub fn is_leading_think_prefix(text: &str) -> bool {
    let leading_ws_len = text.len() - text.trim_start().len();
    let after_ws = &text[leading_ws_len..];
    after_ws.is_empty() || THINK_OPEN_TAG.starts_with(after_ws)
}

pub fn split_at_think_close(text: &str) -> Option<(String, String)> {
    let close_start = text.find(THINK_CLOSE_TAG)?;
    let answer_start = close_start + THINK_CLOSE_TAG.len();
    Some((
        text[..close_start].to_string(),
        strip_think_answer_separator(&text[answer_start..]).to_string(),
    ))
}

pub fn split_incomplete_think_close_suffix(text: &str) -> (&str, &str) {
    for len in (1..THINK_CLOSE_TAG.len()).rev() {
        if text.ends_with(&THINK_CLOSE_TAG[..len]) {
            let split = text.len() - len;
            return (&text[..split], &text[split..]);
        }
    }
    (text, "")
}

fn strip_think_answer_separator(text: &str) -> &str {
    text.trim_start_matches(['\r', '\n', '\t', ' '])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── reasoning_text: reasoning_content (plain string) ──

    #[test]
    fn reasoning_text_from_reasoning_content_string() {
        let msg = json!({"reasoning_content": "step 1 then step 2"});
        assert_eq!(reasoning_text(&msg).unwrap(), "step 1 then step 2");
    }

    #[test]
    fn reasoning_text_from_thinking_string() {
        let msg = json!({"thinking": "hmm let me think"});
        assert_eq!(reasoning_text(&msg).unwrap(), "hmm let me think");
    }

    #[test]
    fn reasoning_text_empty_string_returns_none() {
        let msg = json!({"reasoning_content": ""});
        assert!(reasoning_text(&msg).is_none());
    }

    // ── reasoning_text: reasoning as string ──

    #[test]
    fn reasoning_text_from_reasoning_string() {
        let msg = json!({"reasoning": "direct reasoning"});
        assert_eq!(reasoning_text(&msg).unwrap(), "direct reasoning");
    }

    // ── reasoning_text: reasoning as object ──

    #[test]
    fn reasoning_text_from_reasoning_object_content() {
        let msg = json!({"reasoning": {"content": "from content field"}});
        assert_eq!(reasoning_text(&msg).unwrap(), "from content field");
    }

    #[test]
    fn reasoning_text_from_reasoning_object_text() {
        let msg = json!({"reasoning": {"text": "from text field"}});
        assert_eq!(reasoning_text(&msg).unwrap(), "from text field");
    }

    #[test]
    fn reasoning_text_from_reasoning_object_summary() {
        let msg = json!({"reasoning": {"summary": "from summary field"}});
        assert_eq!(reasoning_text(&msg).unwrap(), "from summary field");
    }

    // ── reasoning_text: reasoning_details ──

    #[test]
    fn reasoning_text_from_reasoning_details_string() {
        let msg = json!({"reasoning_details": "simple details"});
        assert_eq!(reasoning_text(&msg).unwrap(), "simple details");
    }

    #[test]
    fn reasoning_text_from_reasoning_details_array() {
        let msg = json!({"reasoning_details": [
            {"text": "part one"},
            {"text": "part two"}
        ]});
        assert_eq!(reasoning_text(&msg).unwrap(), "part one\n\npart two");
    }

    #[test]
    fn reasoning_text_from_reasoning_details_object() {
        let msg = json!({"reasoning_details": {"text": "single detail"}});
        assert_eq!(reasoning_text(&msg).unwrap(), "single detail");
    }

    #[test]
    fn reasoning_text_from_reasoning_details_content_key() {
        let msg = json!({"reasoning_details": {"content": "via content key"}});
        assert_eq!(reasoning_text(&msg).unwrap(), "via content key");
    }

    // ── reasoning_text: priority ──

    #[test]
    fn reasoning_text_prefers_reasoning_content_over_reasoning() {
        let msg = json!({
            "reasoning_content": "from reasoning_content",
            "reasoning": "from reasoning"
        });
        assert_eq!(reasoning_text(&msg).unwrap(), "from reasoning_content");
    }

    #[test]
    fn reasoning_text_no_reasoning_returns_none() {
        let msg = json!({"content": "just a normal message"});
        assert!(reasoning_text(&msg).is_none());
    }

    #[test]
    fn split_leading_think_block_extracts_reasoning_and_answer() {
        let text = format!("  {THINK_OPEN_TAG} hidden reasoning {THINK_CLOSE_TAG}\n\nfinal answer");
        let (reasoning, answer) = split_leading_think_block(&text).unwrap();
        assert_eq!(reasoning, "hidden reasoning");
        assert_eq!(answer, "final answer");
    }

    #[test]
    fn split_leading_think_block_ignores_non_leading_block() {
        let text = format!("answer first {THINK_OPEN_TAG}hidden{THINK_CLOSE_TAG}");
        assert!(split_leading_think_block(&text).is_none());
    }

    #[test]
    fn leading_think_prefix_detects_partial_open_tag() {
        assert!(is_leading_think_prefix("  <th"));
        assert!(!is_leading_think_prefix("  hello"));
    }

    #[test]
    fn split_incomplete_think_close_suffix_keeps_partial_close_tag() {
        let (emit, keep) = split_incomplete_think_close_suffix("reasoning</thi");
        assert_eq!(emit, "reasoning");
        assert_eq!(keep, "</thi");
    }
}
