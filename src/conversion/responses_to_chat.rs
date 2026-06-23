use crate::state::StoredResponse;
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use super::text::{arguments_text, as_text, compact_json};
use super::tool_context::{custom_tool_input_field, ToolContext};

/// Return type for `build_chat_payload`: (payload, messages, reverse_names, tool_context).
pub type ChatPayload = (Value, Vec<Value>, std::collections::HashMap<String, String>, ToolContext);

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("{0}")]
    Invalid(String),
}

pub fn function_output_call_ids(body: &Value) -> Result<Vec<String>, HistoryError> {
    let (_messages, outputs) = extract_request(body)?;
    Ok(outputs
        .into_iter()
        .filter_map(|output| {
            output
                .get("tool_call_id")
                .or_else(|| output.get("call_id"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect())
}

pub fn build_chat_payload(
    body: &Value,
    model_upstream: &str,
    previous: Option<&StoredResponse>,
    reasoning_parameter: Value,
) -> Result<ChatPayload, HistoryError> {
    let context = ToolContext::build_with_input(body.get("tools"), body.get("input"));
    let (incoming, outputs) = extract_request_with_context(body, &context)?;
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
        ("max_output_tokens", "max_completion_tokens"),
        ("max_tokens", "max_tokens"),
        ("max_completion_tokens", "max_completion_tokens"),
        ("presence_penalty", "presence_penalty"),
        ("frequency_penalty", "frequency_penalty"),
        ("response_format", "response_format"),
        ("seed", "seed"),
        ("stop", "stop"),
        ("logit_bias", "logit_bias"),
        ("logprobs", "logprobs"),
        ("metadata", "metadata"),
        ("n", "n"),
        ("service_tier", "service_tier"),
        ("stream_options", "stream_options"),
        ("top_logprobs", "top_logprobs"),
        ("user", "user"),
    ] {
        if let Some(value) = body.get(source) {
            payload[target] = value.clone();
        }
    }

    if payload.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        let stream_opts = payload.get("stream_options").cloned().unwrap_or_else(|| json!({}));
        if let Some(obj) = stream_opts.as_object() {
            let mut merged = obj.clone();
            merged.insert("include_usage".to_string(), json!(true));
            payload["stream_options"] = Value::Object(merged);
        } else {
            payload["stream_options"] = json!({"include_usage": true});
        }
    }
    if let Some(obj) = reasoning_parameter.as_object() {
        for (key, value) in obj {
            payload[key] = value.clone();
        }
    }

    Ok((payload, messages, context.reverse_names.clone(), context))
}

pub fn extract_request(body: &Value) -> Result<(Vec<Value>, Vec<Value>), HistoryError> {
    let context = ToolContext::build_with_input(body.get("tools"), body.get("input"));
    extract_request_with_context(body, &context)
}

fn extract_request_with_context(body: &Value, context: &ToolContext) -> Result<(Vec<Value>, Vec<Value>), HistoryError> {
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
        object @ Value::Object(_) => vec![object],
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
            "function_call_output" | "custom_tool_call_output" | "tool_search_output" => {
                let call_id = obj.get("call_id").and_then(Value::as_str).unwrap_or("");
                if call_id.is_empty() {
                    return Err(HistoryError::Invalid(format!("{kind} requires call_id")));
                }
                flush_pending(&mut messages, &mut pending_calls);
                let content = if kind == "function_call_output" {
                    let empty = Value::String(String::new());
                    as_text(obj.get("output").unwrap_or(&empty))
                } else {
                    compact_json(&Value::Object(obj.clone()))
                };
                tool_outputs.push(json!({
                    "role":"tool",
                    "tool_call_id":call_id,
                    "content":content,
                }));
            }
            "function_call" => {
                let call_id = obj.get("call_id").or_else(|| obj.get("id")).and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple()));
                let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() {
                    tracing::warn!("skipping function_call without name in request history");
                    continue;
                }
                let chat_name = context.chat_name_for_response_function(name, obj.get("namespace").and_then(Value::as_str));
                pending_calls.push(json!({
                    "id": call_id,
                    "type":"function",
                    "function":{
                        "name": chat_name,
                        "arguments": arguments_text(obj.get("arguments")),
                    }
                }));
            }
            "custom_tool_call" => {
                let call_id = obj.get("call_id").or_else(|| obj.get("id")).and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple()));
                let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() {
                    tracing::warn!("skipping custom_tool_call without name in request history");
                    continue;
                }
                let input = obj.get("input").cloned().unwrap_or_else(|| json!(""));
                pending_calls.push(json!({
                    "id": call_id,
                    "type":"function",
                    "function":{
                        "name": context.chat_name_for_response_function(name, None),
                        "arguments": compact_json(&json!({ custom_tool_input_field(): input })),
                    }
                }));
            }
            "tool_search_call" => {
                let call_id = obj.get("call_id").or_else(|| obj.get("id")).and_then(Value::as_str).map(ToString::to_string).unwrap_or_else(|| format!("call_{}", Uuid::new_v4().simple()));
                let arguments = obj.get("arguments").map(compact_json).unwrap_or_else(|| "{}".to_string());
                pending_calls.push(json!({
                    "id": call_id,
                    "type":"function",
                    "function":{
                        "name": "tool_search",
                        "arguments": arguments,
                    }
                }));
            }
            "reasoning" | "summary" => {
                if let Some(summary_text) = obj.get("summary").and_then(Value::as_array).and_then(|a| a.first()).and_then(|s| s.get("text")).and_then(Value::as_str) {
                    if !summary_text.is_empty() {
                        flush_pending(&mut messages, &mut pending_calls);
                        messages.push(json!({"role":"assistant","reasoning_content":summary_text,"content":""}));
                    }
                }
            }
            "message" | "" => {
                flush_pending(&mut messages, &mut pending_calls);
                let mut role = obj.get("role").and_then(Value::as_str).unwrap_or("user").to_string();
                if role == "developer" { role = "system".to_string(); }
                if !matches!(role.as_str(), "system" | "user" | "assistant" | "tool") { role = "user".to_string(); }
                let empty = Value::String(String::new());
                messages.push(json!({"role":role,"content":as_text(obj.get("content").unwrap_or(&empty))}));
            }
            "input_text" | "output_text" | "text" => {
                flush_pending(&mut messages, &mut pending_calls);
                messages.push(json!({"role":"user","content":as_text(&Value::Object(obj.clone()))}));
            }
            "input_image" => {
                flush_pending(&mut messages, &mut pending_calls);
                let image_url = obj.get("image_url").cloned().unwrap_or_else(|| {
                    obj.get("url").and_then(Value::as_str).map(|u| json!(u)).unwrap_or(Value::Null)
                });
                let image_url = if image_url.is_object() { image_url } else { json!({"url": image_url}) };
                messages.push(json!({"role":"user","content":[{"type":"image_url","image_url":image_url}]}));
            }
            "input_file" => {
                flush_pending(&mut messages, &mut pending_calls);
                messages.push(json!({"role":"user","content":[{"type":"file","file":obj.get("file").cloned().unwrap_or_else(|| Value::Object(obj.clone()))}]}));
            }
            "input_audio" => {
                flush_pending(&mut messages, &mut pending_calls);
                messages.push(json!({"role":"user","content":[{"type":"input_audio","input_audio":obj.get("input_audio").cloned().unwrap_or_else(|| Value::Object(obj.clone()))}]}));
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
        let chat_name = context.chat_name_for_response_function(name, obj.get("namespace").and_then(Value::as_str));
        return Some(json!({"type":"function","function":{"name":chat_name}}));
    }
    if kind == "custom" {
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        return Some(json!({"type":"function","function":{"name":context.chat_name_for_response_function(name, None)}}));
    }
    if kind == "tool_search" {
        return Some(json!({"type":"function","function":{"name":"tool_search"}}));
    }
    Some(choice.clone())
}
