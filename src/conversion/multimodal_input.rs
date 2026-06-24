use serde_json::{json, Map, Value};

pub fn chat_content_from_response_content(content: &Value) -> Option<Value> {
    match content {
        Value::String(text) => Some(Value::String(text.clone())),
        Value::Array(parts) => {
            let chat_parts = parts
                .iter()
                .filter_map(response_content_part_to_chat_part)
                .collect::<Vec<_>>();
            (!chat_parts.is_empty()).then(|| Value::Array(chat_parts))
        }
        Value::Object(_) => response_content_part_to_chat_part(content).map(|part| Value::Array(vec![part])),
        _ => None,
    }
}

pub fn response_content_part_to_chat_part(part: &Value) -> Option<Value> {
    match part {
        Value::String(text) => Some(json!({"type":"text","text":text})),
        Value::Object(obj) => match obj.get("type").and_then(Value::as_str).unwrap_or("") {
            "input_text" | "output_text" | "text" => {
                Some(json!({"type":"text","text":text_from_part(part)}))
            }
            "input_image" | "image" => image_to_chat_content_part(part),
            "input_file" => file_to_chat_content_part(part),
            "input_audio" => audio_to_chat_content_part(part),
            _ => None,
        },
        _ => None,
    }
}

pub fn image_to_chat_content_part(part: &Value) -> Option<Value> {
    image_url_object_from_part(part).map(|image_url| json!({"type":"image_url","image_url":image_url}))
}

pub fn file_to_chat_content_part(part: &Value) -> Option<Value> {
    responses_input_file_to_chat_file(part).map(|file| json!({"type":"file","file":file}))
}

pub fn audio_to_chat_content_part(part: &Value) -> Option<Value> {
    responses_input_audio_to_chat_audio(part)
        .map(|input_audio| json!({"type":"input_audio","input_audio":input_audio}))
}

fn text_from_part(part: &Value) -> String {
    part.get("text")
        .or_else(|| part.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn image_url_object_from_part(part: &Value) -> Option<Value> {
    if let Some(image_url) = part.get("image_url") {
        return Some(if image_url.is_object() {
            image_url.clone()
        } else {
            json!({"url": image_url})
        });
    }
    if let Some(url) = part.get("url") {
        return Some(if url.is_object() {
            url.clone()
        } else {
            json!({"url": url})
        });
    }
    source_image_to_data_url(part)
}

fn source_image_to_data_url(part: &Value) -> Option<Value> {
    let source = part.get("source")?.as_object()?;
    let source_type = source.get("type").and_then(Value::as_str)?;
    if source_type != format!("{}{}", "base", "64") {
        return None;
    }
    let data = source
        .get("data")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    let media_type = source
        .get("media_type")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("image/png");
    Some(json!({"url": format!("{}{};{}{}{}", "data:", media_type, "base", "64,", data)}))
}

fn responses_input_file_to_chat_file(part: &Value) -> Option<Value> {
    let file_source = part.get("file").unwrap_or(part);
    let source_obj = file_source.as_object()?;
    let has_file_payload = source_obj.get("file_data").is_some()
        || source_obj.get("file_id").is_some()
        || part.get("file_data").is_some()
        || part.get("file_id").is_some();
    if !has_file_payload {
        return None;
    }

    let mut file = Map::new();
    for key in ["file_id", "file_data", "filename", "mime_type"] {
        if let Some(value) = source_obj.get(key).or_else(|| part.get(key)) {
            file.insert(key.to_string(), value.clone());
        }
    }
    (!file.is_empty()).then(|| Value::Object(file))
}

fn responses_input_audio_to_chat_audio(part: &Value) -> Option<Value> {
    if let Some(input_audio) = part.get("input_audio") {
        return Some(input_audio.clone());
    }
    let obj = part.as_object()?;
    if !(obj.get("data").is_some() || obj.get("format").is_some()) {
        return None;
    }
    let mut audio = obj.clone();
    audio.remove("type");
    Some(Value::Object(audio))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_base64_source_image_to_data_url() {
        let part = json!({
            "type":"image",
            "source":{"type":"base64","media_type":"image/png","data":"abc"}
        });
        let out = image_to_chat_content_part(&part).unwrap();
        assert_eq!(out["type"], "image_url");
        assert_eq!(out["image_url"]["url"], "data:image/png;base64,abc");
    }

    #[test]
    fn skips_url_only_input_file() {
        let part = json!({"type":"input_file","file":{"url":"https://example.com/a.pdf"}});
        assert!(file_to_chat_content_part(&part).is_none());
    }
}
