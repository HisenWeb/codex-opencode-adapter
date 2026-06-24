pub mod chat_to_responses;
pub mod multimodal_input;
pub mod responses_to_chat;
pub mod stream_chat_to_responses;
pub mod text;
pub mod tool_context;

use serde_json::Value;
use std::collections::HashSet;

pub use chat_to_responses::build_response;
pub use responses_to_chat::{build_chat_payload, HistoryError};
pub use stream_chat_to_responses::StreamAssembler;

/// Return tool output call IDs that require stored-state lookup.
///
/// When Codex sends a full stateless history containing both the original
/// tool-call item and its matching tool output, the server must not preemptively
/// fail on missing `previous_response_id`. Returning an empty lookup set lets
/// `build_chat_payload()` enter its existing stateless repair path.
pub fn function_output_call_ids(body: &Value) -> Result<Vec<String>, HistoryError> {
    let ids = responses_to_chat::function_output_call_ids(body)?;
    if ids.is_empty() || has_previous_response_id(body) {
        return Ok(ids);
    }

    if input_contains_all_tool_calls(body, &ids) {
        tracing::debug!(
            event = "stateless_tool_history_bypass_state_lookup",
            call_ids = ?ids,
            "Responses input contains matching tool calls for every tool output; using stateless history repair instead of stored-state lookup"
        );
        Ok(Vec::new())
    } else {
        Ok(ids)
    }
}

fn has_previous_response_id(body: &Value) -> bool {
    body.get("previous_response_id")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}

fn input_contains_all_tool_calls(body: &Value, ids: &[String]) -> bool {
    let Some(input) = body.get("input") else {
        return false;
    };
    let mut call_ids = HashSet::new();
    collect_tool_call_ids(input, &mut call_ids);
    ids.iter().all(|id| call_ids.contains(id.as_str()))
}

fn collect_tool_call_ids(value: &Value, call_ids: &mut HashSet<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tool_call_ids(item, call_ids);
            }
        }
        Value::Object(obj) => {
            let kind = obj.get("type").and_then(Value::as_str).unwrap_or("");
            if matches!(
                kind,
                "function_call" | "custom_tool_call" | "tool_search_call"
            ) {
                if let Some(call_id) = obj
                    .get("call_id")
                    .or_else(|| obj.get("id"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    call_ids.insert(call_id.to_string());
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::function_output_call_ids;
    use serde_json::json;

    #[test]
    fn self_contained_tool_output_does_not_require_state_lookup() {
        let body = json!({
            "input": [
                {"type":"function_call","call_id":"call_1","name":"run","arguments":"{}"},
                {"type":"function_call_output","call_id":"call_1","output":"ok"}
            ]
        });

        let ids = function_output_call_ids(&body).unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn orphan_tool_output_still_requires_state_lookup() {
        let body = json!({
            "input": [
                {"type":"function_call_output","call_id":"call_1","output":"ok"}
            ]
        });

        let ids = function_output_call_ids(&body).unwrap();
        assert_eq!(ids, vec!["call_1".to_string()]);
    }

    #[test]
    fn previous_response_id_keeps_stored_state_validation() {
        let body = json!({
            "previous_response_id": "resp_prev",
            "input": [
                {"type":"function_call","call_id":"call_1","name":"run","arguments":"{}"},
                {"type":"function_call_output","call_id":"call_1","output":"ok"}
            ]
        });

        let ids = function_output_call_ids(&body).unwrap();
        assert_eq!(ids, vec!["call_1".to_string()]);
    }
}
