use crate::state::{now_ts, StoredResponse};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

use super::chat_to_responses::{completion_status, message_item, reasoning_item, response_shell};
use super::responses_to_chat::repair_history;
use super::text::{arguments_text, as_text, canonicalize_json_string_if_parseable, reasoning_text};

pub type EmitFn = Box<dyn FnMut(&str, Value) -> anyhow::Result<()> + Send>;

#[derive(Debug, Clone)]
struct StreamingToolCall {
    output_index: Option<u32>,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    added: bool,
    done: bool,
}

impl StreamingToolCall {
    fn new() -> Self {
        Self {
            output_index: None,
            item_id: format!("fc_{}", Uuid::new_v4().simple()),
            call_id: String::new(),
            name: String::new(),
            arguments: String::new(),
            added: false,
            done: false,
        }
    }
}

pub struct StreamAssembler {
    body: Value,
    model_alias: String,
    model_upstream: String,
    base_messages: Vec<Value>,
    reverse_names: HashMap<String, String>,
    state_put: Box<dyn FnMut(StoredResponse) -> anyhow::Result<()> + Send>,
    emit: EmitFn,
    response_id: String,
    created_at: i64,
    content: String,
    reasoning: String,
    tool_calls: BTreeMap<usize, StreamingToolCall>,
    usage: Value,
    finish_reason: Option<String>,
    sequence: u64,
    next_output_index: u32,
    text_output_index: Option<u32>,
    reasoning_output_index: Option<u32>,
    message_item_id: String,
    reasoning_item_id: String,
    text_done: bool,
    reasoning_done: bool,
    terminal_emitted: bool,
}

impl StreamAssembler {
    pub fn new(
        body: Value,
        model_alias: String,
        model_upstream: String,
        base_messages: Vec<Value>,
        reverse_names: HashMap<String, String>,
        state_put: Box<dyn FnMut(StoredResponse) -> anyhow::Result<()> + Send>,
        emit: EmitFn,
    ) -> Self {
        Self {
            body,
            model_alias,
            model_upstream,
            base_messages,
            reverse_names,
            state_put,
            emit,
            response_id: format!("resp_{}", Uuid::new_v4().simple()),
            created_at: now_ts(),
            content: String::new(),
            reasoning: String::new(),
            tool_calls: Default::default(),
            usage: json!({}),
            finish_reason: None,
            sequence: 0,
            next_output_index: 0,
            text_output_index: None,
            reasoning_output_index: None,
            message_item_id: format!("msg_{}", Uuid::new_v4().simple()),
            reasoning_item_id: format!("rs_{}", Uuid::new_v4().simple()),
            text_done: false,
            reasoning_done: false,
            terminal_emitted: false,
        }
    }

    pub fn start(&mut self) -> anyhow::Result<()> {
        let response = response_shell(&self.body, &self.response_id, self.created_at, &self.model_alias, vec![], &json!({}), "in_progress", None);
        self.emit_event("response.created", json!({"type":"response.created","response":response}))?;
        let response = response_shell(&self.body, &self.response_id, self.created_at, &self.model_alias, vec![], &json!({}), "in_progress", None);
        self.emit_event("response.in_progress", json!({"type":"response.in_progress","response":response}))
    }

    pub fn accept(&mut self, chunk: &Value) -> anyhow::Result<()> {
        if let Some(usage) = chunk.get("usage").filter(|v| !v.is_null()) {
            self.usage = usage.clone();
        }
        let Some(choices) = chunk.get("choices").and_then(Value::as_array) else {
            return Ok(());
        };
        for choice in choices {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            if let Some(text) = reasoning_text(delta) {
                self.push_reasoning_delta(&text)?;
            }
            if let Some(content) = delta.get("content") {
                let text = as_text(content);
                if !text.is_empty() {
                    self.push_text_delta(&text)?;
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                if !self.reasoning.is_empty() && !self.reasoning_done {
                    self.finish_reasoning_item()?;
                }
                for call in calls {
                    self.accept_tool_delta(call)?;
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(reason.to_string());
            }
        }
        Ok(())
    }

    pub fn finalize(&mut self) -> anyhow::Result<Value> {
        if self.terminal_emitted {
            return Ok(json!({}));
        }
        if !self.reasoning.is_empty() {
            self.finish_reasoning_item()?;
        }
        if !self.content.is_empty() {
            self.finish_text_item()?;
        }
        let mut output = Vec::new();
        let mut assistant = json!({"role":"assistant","content":self.content.clone()});
        if !self.reasoning.is_empty() {
            assistant["reasoning_content"] = Value::String(self.reasoning.clone());
            output.push(reasoning_item(&self.reasoning, Some(self.reasoning_item_id.clone())));
        }
        if !self.content.is_empty() {
            output.push(message_item(&self.content, Some(self.message_item_id.clone())));
        }

        let mut pending = Vec::new();
        let mut replay_calls = Vec::new();
        let keys: Vec<usize> = self.tool_calls.keys().copied().collect();
        for key in keys {
            if self.tool_calls.get(&key).map(|call| call.done).unwrap_or(true) {
                continue;
            }
            if self.tool_calls.get(&key).map(|call| call.name.is_empty()).unwrap_or(true) {
                if let Some(call) = self.tool_calls.get_mut(&key) {
                    call.done = true;
                }
                tracing::warn!(index = key, "skipping streaming tool call with missing name");
                continue;
            }

            if self.tool_calls.get(&key).map(|call| !call.added).unwrap_or(false) {
                let output_index = self.allocate_output_index();
                let (item_id, call_id, raw_name) = {
                    let call = self.tool_calls.get_mut(&key).expect("tool call exists");
                    if call.call_id.is_empty() {
                        call.call_id = format!("call_{key}");
                    }
                    call.output_index = Some(output_index);
                    call.added = true;
                    (call.item_id.clone(), call.call_id.clone(), call.name.clone())
                };
                let restored_name = self.reverse_names.get(&raw_name).cloned().unwrap_or(raw_name);
                let item = json!({"type":"function_call","id":item_id,"call_id":call_id,"name":restored_name,"arguments":"","status":"in_progress"});
                self.emit_event("response.output_item.added", json!({"type":"response.output_item.added","output_index":output_index,"item":item}))?;
            }

            let needs_output_index = self.tool_calls.get(&key).and_then(|call| call.output_index).is_none();
            let assigned_output_index = if needs_output_index {
                Some(self.allocate_output_index())
            } else {
                None
            };
            let (output_index, item_id, call_id, raw_name, arguments) = {
                let call = self.tool_calls.get_mut(&key).expect("tool call exists");
                if call.call_id.is_empty() {
                    call.call_id = format!("call_{key}");
                }
                if let Some(output_index) = assigned_output_index {
                    call.output_index = Some(output_index);
                }
                let output_index = call.output_index.unwrap_or(0);
                let arguments = canonicalize_json_string_if_parseable(&call.arguments);
                call.done = true;
                (
                    output_index,
                    call.item_id.clone(),
                    call.call_id.clone(),
                    call.name.clone(),
                    arguments,
                )
            };
            replay_calls.push(json!({"id":call_id.clone(),"type":"function","function":{"name":raw_name.clone(),"arguments":arguments.clone()}}));
            pending.push(call_id.clone());
            let restored_name = self.reverse_names.get(&raw_name).cloned().unwrap_or(raw_name);
            let item = json!({"type":"function_call","id":item_id.clone(),"call_id":call_id.clone(),"name":restored_name,"arguments":arguments.clone(),"status":"completed"});
            self.emit_event("response.function_call_arguments.done", json!({"type":"response.function_call_arguments.done","output_index":output_index,"item_id":item_id,"call_id":call_id,"arguments":arguments}))?;
            self.emit_event("response.output_item.done", json!({"type":"response.output_item.done","output_index":output_index,"item":item.clone()}))?;
            output.push(item);
        }
        if !replay_calls.is_empty() {
            assistant["tool_calls"] = Value::Array(replay_calls);
        }

        let mut stored_messages = repair_history(&self.base_messages, None)?;
        stored_messages.push(assistant);
        (self.state_put)(StoredResponse {
            response_id: self.response_id.clone(),
            model_alias: self.model_alias.clone(),
            model_upstream: self.model_upstream.clone(),
            messages: stored_messages,
            pending_call_ids: pending.clone(),
            output: output.clone(),
            created_at: self.created_at,
            previous_response_id: self.body.get("previous_response_id").and_then(Value::as_str).unwrap_or("").to_string(),
        })?;
        let finish_value = self.finish_reason.as_ref().map(|reason| Value::String(reason.clone()));
        let (status, incomplete) = completion_status(&self.content, &pending, finish_value.as_ref());
        let response = response_shell(&self.body, &self.response_id, self.created_at, &self.model_alias, output, &self.usage, status, incomplete);
        let event = if status == "completed" { "response.completed" } else { "response.incomplete" };
        self.emit_event(event, json!({"type":event,"response":response.clone()}))?;
        self.terminal_emitted = true;
        Ok(response)
    }

    pub fn fail(&mut self, error_type: &str, message: &str) -> anyhow::Result<Value> {
        if self.terminal_emitted {
            return Ok(json!({}));
        }
        let mut response = response_shell(&self.body, &self.response_id, self.created_at, &self.model_alias, vec![], &self.usage, "failed", None);
        response["error"] = json!({"type":error_type,"message":message.chars().take(1000).collect::<String>()});
        self.emit_event("response.failed", json!({"type":"response.failed","response":response.clone()}))?;
        self.terminal_emitted = true;
        Ok(response)
    }

    fn push_reasoning_delta(&mut self, delta: &str) -> anyhow::Result<()> {
        if self.reasoning_output_index.is_none() {
            let index = self.allocate_output_index();
            self.reasoning_output_index = Some(index);
            self.emit_event("response.output_item.added", json!({"type":"response.output_item.added","output_index":index,"item":{"id":self.reasoning_item_id.clone(),"type":"reasoning","status":"in_progress","summary":[]}}))?;
            self.emit_event("response.reasoning_summary_part.added", json!({"type":"response.reasoning_summary_part.added","item_id":self.reasoning_item_id.clone(),"output_index":index,"summary_index":0,"part":{"type":"summary_text","text":""}}))?;
        }
        self.reasoning.push_str(delta);
        self.emit_event("response.reasoning_summary_text.delta", json!({"type":"response.reasoning_summary_text.delta","item_id":self.reasoning_item_id.clone(),"output_index":self.reasoning_output_index,"summary_index":0,"delta":delta}))
    }

    fn finish_reasoning_item(&mut self) -> anyhow::Result<()> {
        if self.reasoning_output_index.is_none() || self.reasoning_done {
            return Ok(());
        }
        let index = self.reasoning_output_index.unwrap();
        let item = reasoning_item(&self.reasoning, Some(self.reasoning_item_id.clone()));
        self.emit_event("response.reasoning_summary_text.done", json!({"type":"response.reasoning_summary_text.done","item_id":self.reasoning_item_id.clone(),"output_index":index,"summary_index":0,"text":self.reasoning.clone()}))?;
        self.emit_event("response.reasoning_summary_part.done", json!({"type":"response.reasoning_summary_part.done","item_id":self.reasoning_item_id.clone(),"output_index":index,"summary_index":0,"part":{"type":"summary_text","text":self.reasoning.clone()}}))?;
        self.emit_event("response.output_item.done", json!({"type":"response.output_item.done","output_index":index,"item":item}))?;
        self.reasoning_done = true;
        Ok(())
    }

    fn push_text_delta(&mut self, text: &str) -> anyhow::Result<()> {
        self.ensure_text_started()?;
        self.content.push_str(text);
        self.emit_event("response.output_text.delta", json!({"type":"response.output_text.delta","output_index":self.text_output_index,"content_index":0,"item_id":self.message_item_id.clone(),"delta":text}))
    }

    fn ensure_text_started(&mut self) -> anyhow::Result<()> {
        if self.text_output_index.is_some() {
            return Ok(());
        }
        if !self.reasoning.is_empty() && !self.reasoning_done {
            self.finish_reasoning_item()?;
        }
        let index = self.allocate_output_index();
        self.text_output_index = Some(index);
        self.emit_event("response.output_item.added", json!({"type":"response.output_item.added","output_index":index,"item":{"type":"message","id":self.message_item_id.clone(),"status":"in_progress","role":"assistant","content":[]}}))?;
        self.emit_event("response.content_part.added", json!({"type":"response.content_part.added","output_index":index,"content_index":0,"item_id":self.message_item_id.clone(),"part":{"type":"output_text","text":"","annotations":[]}}))
    }

    fn finish_text_item(&mut self) -> anyhow::Result<()> {
        if self.text_output_index.is_none() || self.text_done {
            return Ok(());
        }
        let index = self.text_output_index.unwrap();
        let item = message_item(&self.content, Some(self.message_item_id.clone()));
        self.emit_event("response.output_text.done", json!({"type":"response.output_text.done","output_index":index,"content_index":0,"item_id":self.message_item_id.clone(),"text":self.content.clone()}))?;
        self.emit_event("response.content_part.done", json!({"type":"response.content_part.done","output_index":index,"content_index":0,"item_id":self.message_item_id.clone(),"part":{"type":"output_text","text":self.content.clone(),"annotations":[]}}))?;
        self.emit_event("response.output_item.done", json!({"type":"response.output_item.done","output_index":index,"item":item}))?;
        self.text_done = true;
        Ok(())
    }

    fn accept_tool_delta(&mut self, delta: &Value) -> anyhow::Result<()> {
        let Some(index) = delta.get("index").and_then(Value::as_u64).map(|value| value as usize) else {
            tracing::warn!("skipping streaming tool call delta without index");
            return Ok(());
        };
        let new_id = delta.get("id").and_then(Value::as_str).filter(|value| !value.is_empty()).map(ToString::to_string);
        let function = delta.get("function").unwrap_or(&Value::Null);
        let name_delta = function
            .get("name")
            .or_else(|| delta.get("name"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let args_delta = function
            .get("arguments")
            .or_else(|| delta.get("arguments"))
            .map(|value| arguments_text(Some(value)))
            .filter(|value| !value.is_empty());

        let mut emit_args: Option<String> = None;
        {
            let entry = self.tool_calls.entry(index).or_insert_with(StreamingToolCall::new);
            if let Some(id) = new_id {
                if entry.added {
                    if entry.call_id.is_empty() {
                        entry.call_id = id;
                    } else if entry.call_id.as_str() != id.as_str() {
                        tracing::warn!(index, existing = %entry.call_id, incoming = %id, "ignoring streaming tool call id change after start");
                    }
                } else {
                    entry.call_id = id;
                }
            }
            if let Some(name) = name_delta {
                if entry.added {
                    if entry.name.as_str() != name.as_str() {
                        tracing::warn!(index, existing = %entry.name, incoming = %name, "ignoring streaming tool call name change after start");
                    }
                } else {
                    entry.name = name;
                }
            }
            if let Some(part) = args_delta {
                let was_added = entry.added;
                entry.arguments.push_str(&part);
                if was_added {
                    emit_args = Some(part);
                }
            }
        }
        self.ensure_tool_started(index)?;
        if let Some(part) = emit_args {
            self.emit_tool_arguments(index, &part)?;
        }
        Ok(())
    }

    fn ensure_tool_started(&mut self, index: usize) -> anyhow::Result<()> {
        let should_start = self
            .tool_calls
            .get(&index)
            .map(|entry| !entry.added && !entry.call_id.is_empty() && !entry.name.is_empty())
            .unwrap_or(false);
        if !should_start {
            return Ok(());
        }
        if !self.content.is_empty() && !self.text_done {
            self.finish_text_item()?;
        }
        if !self.reasoning.is_empty() && !self.reasoning_done {
            self.finish_reasoning_item()?;
        }
        let output_index = self.allocate_output_index();
        let (item_id, call_id, raw_name, pending_arguments) = {
            let entry = self.tool_calls.get_mut(&index).expect("tool call exists");
            entry.output_index = Some(output_index);
            entry.added = true;
            (
                entry.item_id.clone(),
                entry.call_id.clone(),
                entry.name.clone(),
                entry.arguments.clone(),
            )
        };
        let restored = self.reverse_names.get(&raw_name).cloned().unwrap_or(raw_name);
        let item = json!({"type":"function_call","id":item_id,"call_id":call_id,"name":restored,"arguments":"","status":"in_progress"});
        self.emit_event("response.output_item.added", json!({"type":"response.output_item.added","output_index":output_index,"item":item}))?;
        if !pending_arguments.is_empty() {
            self.emit_tool_arguments(index, &pending_arguments)?;
        }
        Ok(())
    }

    fn emit_tool_arguments(&mut self, index: usize, part: &str) -> anyhow::Result<()> {
        if part.is_empty() {
            return Ok(());
        }
        let Some(entry) = self.tool_calls.get(&index) else {
            tracing::warn!(index, "cannot emit arguments for missing streaming tool call");
            return Ok(());
        };
        if !entry.added {
            tracing::warn!(index, "cannot emit arguments before streaming tool call start");
            return Ok(());
        }
        let Some(output_index) = entry.output_index else {
            tracing::warn!(index, "cannot emit arguments without output_index");
            return Ok(());
        };
        self.emit_event("response.function_call_arguments.delta", json!({"type":"response.function_call_arguments.delta","output_index":output_index,"item_id":entry.item_id.clone(),"call_id":entry.call_id.clone(),"delta":part}))
    }

    fn allocate_output_index(&mut self) -> u32 {
        let value = self.next_output_index;
        self.next_output_index += 1;
        value
    }

    fn emit_event(&mut self, event: &str, mut payload: Value) -> anyhow::Result<()> {
        self.sequence += 1;
        payload["response_id"] = Value::String(self.response_id.clone());
        payload["sequence_number"] = Value::from(self.sequence);
        (self.emit)(event, payload)
    }
}
