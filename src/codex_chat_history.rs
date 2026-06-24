use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::state::StoredResponse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryResolutionError {
    UnknownToolCall { call_ids: Vec<String> },
    AmbiguousToolCall { call_ids: Vec<String> },
    DuplicateToolOutput { call_id: String },
    SplitToolOutputs { call_ids: Vec<String> },
    InvalidToolHistory { reason: String },
}

impl std::fmt::Display for HistoryResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownToolCall { call_ids } => {
                write!(f, "unknown tool call id(s): {}", call_ids.join(", "))
            }
            Self::AmbiguousToolCall { call_ids } => {
                write!(f, "ambiguous tool call id(s): {}", call_ids.join(", "))
            }
            Self::DuplicateToolOutput { call_id } => {
                write!(f, "duplicate tool output for call id: {call_id}")
            }
            Self::SplitToolOutputs { call_ids } => {
                write!(
                    f,
                    "tool outputs are split across multiple stored responses: {}",
                    call_ids.join(", ")
                )
            }
            Self::InvalidToolHistory { reason } => write!(f, "invalid tool history: {reason}"),
        }
    }
}

impl std::error::Error for HistoryResolutionError {}

pub type HistoryResult<T> = Result<T, HistoryResolutionError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryLookup {
    NoToolOutputs,
    ByPreviousResponseId(String),
    ByUniqueCallIds(Vec<String>),
}

pub fn resolve_history<'a>(
    previous_response_id: Option<&str>,
    requested_call_ids: &[String],
    candidates: &'a [StoredResponse],
) -> HistoryResult<Option<&'a StoredResponse>> {
    ensure_no_duplicate_call_outputs(requested_call_ids)?;

    if let Some(previous_response_id) = previous_response_id.filter(|value| !value.is_empty()) {
        return Ok(candidates
            .iter()
            .find(|item| item.response_id == previous_response_id));
    }

    if requested_call_ids.is_empty() {
        return Ok(None);
    }

    let mut matching_response_id: Option<&str> = None;
    let mut found_call_ids = HashSet::new();

    for requested in requested_call_ids {
        let mut matched_this_call: Option<&str> = None;
        for candidate in candidates {
            if candidate.pending_call_ids.iter().any(|call_id| call_id == requested) {
                match matched_this_call {
                    Some(existing) if existing != candidate.response_id => {
                        return Err(HistoryResolutionError::AmbiguousToolCall {
                            call_ids: vec![requested.clone()],
                        });
                    }
                    Some(_) => {}
                    None => matched_this_call = Some(candidate.response_id.as_str()),
                }
            }
        }

        let Some(matched_response_id) = matched_this_call else {
            return Err(HistoryResolutionError::UnknownToolCall {
                call_ids: vec![requested.clone()],
            });
        };
        found_call_ids.insert(requested.as_str());

        match matching_response_id {
            Some(existing) if existing != matched_response_id => {
                return Err(HistoryResolutionError::SplitToolOutputs {
                    call_ids: requested_call_ids.to_vec(),
                });
            }
            Some(_) => {}
            None => matching_response_id = Some(matched_response_id),
        }
    }

    if found_call_ids.len() != requested_call_ids.len() {
        return Err(HistoryResolutionError::UnknownToolCall {
            call_ids: requested_call_ids.to_vec(),
        });
    }

    Ok(matching_response_id.and_then(|response_id| {
        candidates
            .iter()
            .find(|item| item.response_id == response_id)
    }))
}

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

pub fn call_ids_by_response(candidates: &[StoredResponse]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for candidate in candidates {
        for call_id in &candidate.pending_call_ids {
            index
                .entry(call_id.clone())
                .or_default()
                .push(candidate.response_id.clone());
        }
    }
    index
}
