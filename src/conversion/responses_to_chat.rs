use crate::state::StoredResponse;
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use super::text::{arguments_text, as_text};
use super::tool_context::ToolContext;

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("{0}")]
    Invalid(String),
}

pub fn function_output_call_ids(body: &Value) -> Result<Vec<String>, HistoryError> {
    let (_messages, outputs) = extract_request(body)?;
    Ok(outputs
        .into_iter()
        .filter_map(|output| output.get("tool_call_id").and_then(Value::as_str).map(ToString::to_string))
        .collect())
}

pub fn build_chat_payload(
    body: &Value,
    model_upstream: &str,
    previous: Option<&StoredResponse>,
    reasoning_parameter: Value,
) -> Result<(Value, Vec<Value>, std::collections::HashMap<String, String>), HistoryError> {
    let (incoming, outputs) = extract_request(body)?;
    let mut messages = if !outputs.is_empty() {
        let previous = previous.ok_or_else(|| HistoryError::Invalid("tool output has no matching stored response".to_string()))?;
        let repaired = repair_history(&previous.messages, Some(&outputs))?;
        merge_new_messages(&repaired, &incoming)
    } else if let Some(previous) = previous {
        merge_new_messages(&previous.messages, &incoming)
    } else {
        incoming
    };
    if messages.is_empty() {
        messages.push(json!({"role":"user","content":""}));
    }
    messages = normalize_upstream_roles(&messages);

    let context = ToolContext::build(body.get("tools"));
    let mut payload = json!({
        "model": model_upstream,
        "messages": messages,
        "stream": body.get("stream").and_then(Value::as_bool).unwrap_or(false),
    });

    if !context.chat_tools.is_empty() {
        payload["tools"] = Value::Array(context.chat_tools.clone());
        if let Some(choice) = convert_tool_choice(body.get("tool_choice"), &context) {
            payload["tool_choice"] = choice;
        }
        if let Some(parallel) = body.get("parallel_tool_calls").and_then(Value::as_bool) {
            payload["parallel_tool_calls"] = Value::Bool(parallel);
        }
    }

    for (source, target) in [
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("max_output_tokens", "max_tokens"),
        ("max_tokens", "max_tokens"),
        ("presence_penalty", "presence_penalty"),
        ("frequency_penalty", "frequency_penalty"),
        ("response_format", "response_format"),
        ("seed", "seed"),
        ("stop", "stop"),
    ] {
        if let Some(value) = body.get(source) {
            payload[target] = value.clone();
        }
    }

    if payload.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        payload["stream_options"] = json!({"include_usage": true});
    }
    if let Some(obj) = reasoning_parameter.as_object() {
        for (key, value) in obj {
            payload[key] = value.clone();
        }
    }

    Ok((payload, messages, context.reverse_names))
}

pub fn extract_request(body: &Value) -> Result<(Vec<Value>, Vec<Value>), HistoryError> {
    let mut messages = Vec::new();
    let mut tool_outputs = Vec::new();

    if let Some(instructions) = body.get("instructions") {
        if !instructions.is_null() {
            messages.push(json!({"role":"system","content":as_text(instructions)}));
        }
    }

    let raw_input = body.get("input").cloned().unwrap_or_else(|| Value::Array(vec![]));
    let items = match raw_input {
        Value::String(text) => vec![json!({"role":"user","content":text})],
        Value::Object(_) => vec![raw_input],
        Value::Array(items) => items,
        _ => return Err(HistoryError::Invalid("input must be a string, object, or list".to_string())),
    };

    let mut pending_calls: Vec<Value> = Vec::new();
    let flush_pending = |messages: &mut Vec<Value>, pending_calls: &mut Vec<Value>| {
        if !pending_calls.is_empty() {
            messages.push(json!({"role":"assistant","content":"","tool_calls":pending_calls.clone()}));
            pending_calls.clear();
        }
    };

    for item in items {
        if let Value::String(text) = item {
            flush_pending(&mut messages, &mut pending_calls);
            messages.push(json!({"role":"user","content":text}));
            continue;
        }
        let Some(obj) = item.as_object() else { continue; };
        let kind = obj.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "function_call_output" => {
                let call_id = obj.get("call_id").and_then(Value::as_str).unwrap_or("");
                if call_id.is_empty() {
                    return Err(HistoryError::Invalid("function_call_output requires call_id".to_string()));
                }
                flush_pending(&mut messages, &mut pending_calls);
                tool_outputs.push(json!({
                    "role":"tool",
                    "tool_call_id":call_id,
                    "content":as_text(obj.get("output").unwrap_or(&Value::String(String::new()))),
                }));
            }
            "function_call" => {
                pending_calls.push(json!({
                    "id": obj.get("call_id").or_else(|| obj.get("id")).and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple())),
                    "type":"function",
                    "function":{
                        "name": obj.get("name").and_then(Value::as_str).unwrap_or("tool"),
                        "arguments": arguments_text(obj.get("arguments")),
                    }
                }));
            }
            "reasoning" | "summary" => {}
            "message" | "" => {
                flush_pending(&mut messages, &mut pending_calls);
                let mut role = obj.get("role").and_then(Value::as_str).unwrap_or("user").to_string();
                if role == "developer" { role = "system".to_string(); }
                if !matches!(role.as_str(), "system" | "user" | "assistant" | "tool") { role = "user".to_string(); }
                messages.push(json!({"role":role,"content":as_text(obj.get("content").unwrap_or(&Value::String(String::new())))}));
            }
            "input_text" | "output_text" | "text" => {
                flush_pending(&mut messages, &mut pending_calls);
                messages.push(json!({"role":"user","content":as_text(&Value::Object(obj.clone()))}));
            }
            _ => {}
        }
    }
    flush_pending(&mut messages, &mut pending_calls);
    Ok((messages, tool_outputs))
}

pub fn merge_new_messages(base: &[Value], incoming: &[Value]) -> Vec<Value> {
    base.iter().cloned().chain(incoming.iter().cloned()).collect()
}

pub fn normalize_upstream_roles(messages: &[Value]) -> Vec<Value> {
    let mut system_chunks = Vec::new();
    let mut rest = Vec::new();
    for message in messages {
        let mut item = message.clone();
        if item.get("role").and_then(Value::as_str) == Some("developer") {
            item["role"] = Value::String("system".to_string());
        }
        if item.get("role").and_then(Value::as_str) == Some("system") {
            let text = item.get("content").map(as_text).unwrap_or_default();
            if !text.is_empty() { system_chunks.push(text); }
            continue;
        }
        if !matches!(item.get("role").and_then(Value::as_str), Some("user" | "assistant" | "tool")) {
            item["role"] = Value::String("user".to_string());
        }
        rest.push(item);
    }
    if !system_chunks.is_empty() {
        let mut out = vec![json!({"role":"system","content":system_chunks.join("\n\n")})];
        out.extend(rest);
        out
    } else {
        rest
    }
}

pub fn repair_history(messages: &[Value], tool_outputs: Option<&[Value]>) -> Result<Vec<Value>, HistoryError> {
    let mut repaired = messages.to_vec();
    let Some(outputs) = tool_outputs else { return Ok(repaired); };
    let pending: std::collections::HashSet<String> = repaired
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
        .flat_map(|m| m.get("tool_calls").and_then(Value::as_array).into_iter().flatten())
        .filter_map(|call| call.get("id").and_then(Value::as_str).map(ToString::to_string))
        .collect();
    let mut seen = std::collections::HashSet::new();
    for output in outputs {
        let call_id = output.get("tool_call_id").and_then(Value::as_str).unwrap_or("");
        if !pending.contains(call_id) {
            return Err(HistoryError::Invalid(format!("unknown tool call id: {call_id}")));
        }
        if !seen.insert(call_id.to_string()) {
            return Err(HistoryError::Invalid(format!("duplicate tool output: {call_id}")));
        }
        repaired.push(output.clone());
    }
    Ok(repaired)
}

fn convert_tool_choice(choice: Option<&Value>, context: &ToolContext) -> Option<Value> {
    let choice = choice?;
    if let Some(text) = choice.as_str() {
        return Some(Value::String(text.to_string()));
    }
    let obj = choice.as_object()?;
    let kind = obj.get("type").and_then(Value::as_str).unwrap_or("");
    if matches!(kind, "auto" | "none" | "required") {
        return Some(Value::String(kind.to_string()));
    }
    if matches!(kind, "function" | "tool") {
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        let chat_name = context
            .reverse_names
            .iter()
            .find_map(|(safe, original)| (original == name).then_some(safe.clone()))
            .unwrap_or_else(|| name.to_string());
        return Some(json!({"type":"function","function":{"name":chat_name}}));
    }
    Some(choice.clone())
}
