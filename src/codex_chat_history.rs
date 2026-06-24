use serde_json::Value;
use std::collections::HashSet;

use crate::state::StoredResponse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryResolutionError {
    UnknownToolCall { call_id: String },
    DuplicateToolOutput { call_id: String },
    InvalidToolHistory { reason: String },
}

impl std::fmt::Display for HistoryResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownToolCall { call_id } => write!(f, "unknown tool call id: {call_id}"),
            Self::DuplicateToolOutput { call_id } => {
                write!(f, "duplicate tool output: {call_id}")
            }
            Self::InvalidToolHistory { reason } => write!(f, "invalid tool history: {reason}"),
        }
    }
}

impl std::error::Error for HistoryResolutionError {}

pub type HistoryResult<T> = Result<T, HistoryResolutionError>;

pub fn ensure_no_duplicate_call_outputs(call_ids: &[String]) -> HistoryResult<()> {
    let mut seen = HashSet::new();
    for call_id in call_ids {
        if !seen.insert(call_id.as_str()) {
            return Err(HistoryResolutionError::DuplicateToolOutput {
                call_id: call_id.clone(),
            });
        }
    }
    Ok(())
}

pub fn validate_requested_call_ids(
    previous: &StoredResponse,
    requested_call_ids: &[String],
) -> HistoryResult<()> {
    ensure_no_duplicate_call_outputs(requested_call_ids)?;
    let pending: HashSet<&str> = previous
        .pending_call_ids
        .iter()
        .map(String::as_str)
        .collect();
    for call_id in requested_call_ids {
        if !pending.contains(call_id.as_str()) {
            return Err(HistoryResolutionError::UnknownToolCall {
                call_id: call_id.clone(),
            });
        }
    }
    Ok(())
}

pub fn validate_chat_tool_history(messages: &[Value]) -> HistoryResult<()> {
    let mut seen_assistant_calls = HashSet::new();
    let mut consumed_tool_outputs = HashSet::new();

    for message in messages {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "assistant" {
            if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                for call in calls {
                    if let Some(call_id) = call
                        .get("id")
                        .or_else(|| call.get("call_id"))
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                    {
                        seen_assistant_calls.insert(call_id.to_string());
                    }
                }
            }
            continue;
        }

        if role == "tool" {
            let call_id = message
                .get("tool_call_id")
                .or_else(|| message.get("call_id"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| HistoryResolutionError::InvalidToolHistory {
                    reason: "tool message is missing tool_call_id".to_string(),
                })?;

            if !seen_assistant_calls.contains(call_id) {
                return Err(HistoryResolutionError::InvalidToolHistory {
                    reason: format!(
                        "tool message references {call_id}, but no preceding assistant tool_call exists"
                    ),
                });
            }
            if !consumed_tool_outputs.insert(call_id.to_string()) {
                return Err(HistoryResolutionError::DuplicateToolOutput {
                    call_id: call_id.to_string(),
                });
            }
        }
    }

    Ok(())
}
