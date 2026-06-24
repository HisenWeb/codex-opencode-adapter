use crate::state::{now_ts, StoredResponse};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::chat_to_responses::{completion_status, message_item, reasoning_item, response_shell};
use super::responses_to_chat::repair_history;
use super::text::{
    arguments_text, as_text, canonicalize_json_string_if_parseable, is_leading_think_prefix,
    reasoning_text, split_at_think_close, split_incomplete_think_close_suffix,
    split_leading_think_block, strip_leading_think_open_tag,
};
use super::tool_context::{ToolContext, ToolKind};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThinkBlockState {
    Detecting,
    InThink,
    Done,
}

pub struct StreamAssembler {
    body: Value,
    model_alias: String,
    model_upstream: String,
    base_messages: Vec<Value>,
    tool_context: ToolContext,
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
    think_state: ThinkBlockState,
    think_buffer: String,
}

impl StreamAssembler {
    pub fn new(
        body: Value,
        model_alias: String,
        model_upstream: String,
        base_messages: Vec<Value>,
        tool_context: ToolContext,
        state_put: Box<dyn FnMut(StoredResponse) -> anyhow::Result<()> + Send>,
        emit: EmitFn,
    ) -> Self {
        Self {
            body,
            model_alias,
            model_upstream,
            base_messages,
            tool_context,
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
            think_state: ThinkBlockState::Detecting,
            think_buffer: String::new(),
        }
    }

    pub fn start(&mut self) -> anyhow::Result<()> {
        let response = response_shell(
            &self.body,
            &self.response_id,
            self.created_at,
            &self.model_alias,
            vec![],
            &json!({}),
            "in_progress",
            None,
        );
        self.emit_event(
            "response.created",
            json!({"type":"response.created","response":response}),
        )?;
        let response = response_shell(
            &self.body,
            &self.response_id,
            self.created_at,
            &self.model_alias,
            vec![],
            &json!({}),
            "in_progress",
            None,
        );
        self.emit_event(
            "response.in_progress",
            json!({"type":"response.in_progress","response":response}),
        )
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
                self.disable_leading_think_detection()?;
                self.push_reasoning_delta(&text)?;
            }
            if let Some(content) = delta.get("content") {
                let text = as_text(content);
                if !text.is_empty() {
                    self.accept_content_delta(&text)?;
                }
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                self.flush_pending_think_buffer()?;
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

    pub fn mark_truncated_as_length(&mut self) {
        if self.finish_reason.is_none() {
            self.finish_reason = Some("length".to_string());
        }
    }

    pub fn finalize(&mut self) -> anyhow::Result<Value> {
        if self.terminal_emitted {
            return Ok(json!({}));
        }
        self.flush_pending_think_buffer()?;
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
            output.push(reasoning_item(
                &self.reasoning,
                Some(self.reasoning_item_id.clone()),
            ));
        }
        if !self.content.is_empty() {
            output.push(message_item(
                &self.content,
                Some(self.message_item_id.clone()),
            ));
        }

        let mut pending = Vec::new();
        let mut replay_calls = Vec::new();
        let keys: Vec<usize> = self.tool_calls.keys().copied().collect();
        for key in keys {
            if self
                .tool_calls
                .get(&key)
                .map(|call| call.done)
                .unwrap_or(true)
            {
                continue;
            }
            if self
                .tool_calls
                .get(&key)
                .map(|call| call.name.is_empty())
                .unwrap_or(true)
            {
                if let Some(call) = self.tool_calls.get_mut(&key) {
                    call.done = true;
                }
                tracing::warn!(
                    index = key,
                    "skipping streaming tool call with missing name"
                );
                continue;
            }

            if self
                .tool_calls
                .get(&key)
                .map(|call| !call.added)
                .unwrap_or(false)
            {
                self.start_tool_for_finalize(key)?;
            }

            let (output_index, item_id, call_id, raw_name, arguments) = {
                let call = self.tool_calls.get_mut(&key).expect("tool call exists");
                if call.call_id.is_empty() {
                    call.call_id = format!("call_{key}");
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
            let item =
                self.response_tool_item(&item_id, "completed", &call_id, &raw_name, &arguments);
            if self.tool_context.is_custom_tool_chat_name(&raw_name) {
                let input = custom_tool_input_from_chat_arguments(&arguments);
                if !input.is_empty() {
                    self.emit_event("response.custom_tool_call_input.delta", json!({"type":"response.custom_tool_call_input.delta","output_index":output_index,"item_id":item_id,"delta":input}))?;
                }
                self.emit_event("response.custom_tool_call_input.done", json!({"type":"response.custom_tool_call_input.done","output_index":output_index,"item_id":item_id,"input":input}))?;
            } else {
                self.emit_event("response.function_call_arguments.done", json!({"type":"response.function_call_arguments.done","output_index":output_index,"item_id":item_id,"call_id":call_id,"arguments":arguments}))?;
            }
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
            previous_response_id: self
                .body
                .get("previous_response_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })?;
        let finish_value = self
            .finish_reason
            .as_ref()
            .map(|reason| Value::String(reason.clone()));
        let (status, incomplete) =
            completion_status(&self.content, &pending, finish_value.as_ref());
        let response = response_shell(
            &self.body,
            &self.response_id,
            self.created_at,
            &self.model_alias,
            output,
            &self.usage,
            status,
            incomplete,
        );
        let event = if status == "completed" {
            "response.completed"
        } else {
            "response.incomplete"
        };
        self.emit_event(event, json!({"type":event,"response":response.clone()}))?;
        self.terminal_emitted = true;
        Ok(response)
    }

    pub fn fail(&mut self, error_type: &str, message: &str) -> anyhow::Result<Value> {
        if self.terminal_emitted {
            return Ok(json!({}));
        }
        let mut response = response_shell(
            &self.body,
            &self.response_id,
            self.created_at,
            &self.model_alias,
            vec![],
            &self.usage,
            "failed",
            None,
        );
        response["error"] =
            json!({"type":error_type,"message":message.chars().take(1000).collect::<String>()});
        self.emit_event(
            "response.failed",
            json!({"type":"response.failed","response":response.clone()}),
        )?;
        self.terminal_emitted = true;
        Ok(response)
    }

    fn start_tool_for_finalize(&mut self, index: usize) -> anyhow::Result<()> {
        let output_index = self.allocate_output_index();
        let (item_id, call_id, raw_name) = {
            let call = self.tool_calls.get_mut(&index).expect("tool call exists");
            if call.call_id.is_empty() {
                call.call_id = format!("call_{index}");
            }
            call.output_index = Some(output_index);
            call.added = true;
            (
                call.item_id.clone(),
                call.call_id.clone(),
                call.name.clone(),
            )
        };
        let item = self.response_tool_item(&item_id, "in_progress", &call_id, &raw_name, "");
        self.emit_event(
            "response.output_item.added",
            json!({"type":"response.output_item.added","output_index":output_index,"item":item}),
        )
    }

    fn disable_leading_think_detection(&mut self) -> anyhow::Result<()> {
        if self.think_state != ThinkBlockState::Done {
            self.flush_pending_think_buffer()?;
            self.think_state = ThinkBlockState::Done;
        }
        Ok(())
    }

    fn accept_content_delta(&mut self, text: &str) -> anyhow::Result<()> {
        match self.think_state {
            ThinkBlockState::Done => self.push_text_delta(text),
            ThinkBlockState::Detecting => {
                self.think_buffer.push_str(text);
                if let Some((reasoning, answer)) = split_leading_think_block(&self.think_buffer) {
                    if !reasoning.is_empty() {
                        self.push_reasoning_delta(&reasoning)?;
                    }
                    self.think_buffer.clear();
                    self.think_state = ThinkBlockState::Done;
                    if !answer.is_empty() {
                        self.push_text_delta(&answer)?;
                    }
                    return Ok(());
                }
                if let Some(reasoning_start) = strip_leading_think_open_tag(&self.think_buffer) {
                    self.think_buffer.clear();
                    self.think_state = ThinkBlockState::InThink;
                    if !reasoning_start.is_empty() {
                        self.accept_in_think_content(&reasoning_start)?;
                    }
                    return Ok(());
                }
                if is_leading_think_prefix(&self.think_buffer) {
                    return Ok(());
                }
                let text = std::mem::take(&mut self.think_buffer);
                self.think_state = ThinkBlockState::Done;
                self.push_text_delta(&text)
            }
            ThinkBlockState::InThink => self.accept_in_think_content(text),
        }
    }

    fn accept_in_think_content(&mut self, text: &str) -> anyhow::Result<()> {
        self.think_buffer.push_str(text);
        if let Some((reasoning, answer)) = split_at_think_close(&self.think_buffer) {
            if !reasoning.is_empty() {
                self.push_reasoning_delta(&reasoning)?;
            }
            self.think_buffer.clear();
            self.think_state = ThinkBlockState::Done;
            if !answer.is_empty() {
                self.push_text_delta(&answer)?;
            }
            return Ok(());
        }

        let (emit, keep) = split_incomplete_think_close_suffix(&self.think_buffer);
        let emit = emit.to_string();
        let keep = keep.to_string();
        if !emit.is_empty() {
            self.push_reasoning_delta(&emit)?;
        }
        self.think_buffer = keep;
        Ok(())
    }

    fn flush_pending_think_buffer(&mut self) -> anyhow::Result<()> {
        match self.think_state {
            ThinkBlockState::Detecting => {
                if !self.think_buffer.is_empty() {
                    let text = std::mem::take(&mut self.think_buffer);
                    self.think_state = ThinkBlockState::Done;
                    self.push_text_delta(&text)?;
                }
            }
            ThinkBlockState::InThink => {
                if !self.think_buffer.is_empty() {
                    let reasoning = std::mem::take(&mut self.think_buffer);
                    self.push_reasoning_delta(&reasoning)?;
                }
                self.think_state = ThinkBlockState::Done;
            }
            ThinkBlockState::Done => {}
        }
        Ok(())
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
        self.emit_event(
            "response.output_item.done",
            json!({"type":"response.output_item.done","output_index":index,"item":item}),
        )?;
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
        self.emit_event(
            "response.output_item.done",
            json!({"type":"response.output_item.done","output_index":index,"item":item}),
        )?;
        self.text_done = true;
        Ok(())
    }

    fn accept_tool_delta(&mut self, delta: &Value) -> anyhow::Result<()> {
        let Some(index) = delta
            .get("index")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
        else {
            tracing::warn!("skipping streaming tool call delta without index");
            return Ok(());
        };
        let new_id = delta
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
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
            let entry = self
                .tool_calls
                .entry(index)
                .or_insert_with(StreamingToolCall::new);
            if let Some(id) = new_id {
                if entry.added && !entry.call_id.is_empty() && entry.call_id != id {
                    tracing::warn!(index, existing = %entry.call_id, incoming = %id, "ignoring streaming tool call id change after start");
                } else if !entry.added || entry.call_id.is_empty() {
                    entry.call_id = id;
                }
            }
            if let Some(name) = name_delta {
                if entry.added && entry.name != name {
                    tracing::warn!(index, existing = %entry.name, incoming = %name, "ignoring streaming tool call name change after start");
                } else if !entry.added {
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
        let item = self.response_tool_item(&item_id, "in_progress", &call_id, &raw_name, "");
        self.emit_event(
            "response.output_item.added",
            json!({"type":"response.output_item.added","output_index":output_index,"item":item}),
        )?;
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
            return Ok(());
        };
        if !entry.added {
            return Ok(());
        }
        if self.tool_context.is_custom_tool_chat_name(&entry.name) {
            return Ok(());
        }
        let Some(output_index) = entry.output_index else {
            return Ok(());
        };
        self.emit_event("response.function_call_arguments.delta", json!({"type":"response.function_call_arguments.delta","output_index":output_index,"item_id":entry.item_id.clone(),"call_id":entry.call_id.clone(),"delta":part}))
    }

    fn response_tool_item(
        &self,
        item_id: &str,
        status: &str,
        call_id: &str,
        chat_name: &str,
        arguments: &str,
    ) -> Value {
        match self.tool_context.lookup_spec(chat_name) {
            Some(spec) if spec.kind == ToolKind::Custom => {
                json!({"id":item_id,"type":"custom_tool_call","status":status,"call_id":call_id,"name":spec.name,"input":custom_tool_input_from_chat_arguments(arguments)})
            }
            Some(spec) if spec.kind == ToolKind::ToolSearch => {
                json!({"type":"tool_search_call","status":status,"call_id":call_id,"execution":"client","arguments":parse_tool_arguments_object(arguments)})
            }
            Some(spec) => {
                let mut item = json!({"id":item_id,"type":"function_call","status":status,"call_id":call_id,"name":spec.name,"arguments":arguments});
                if let Some(namespace) = spec.namespace.as_deref().filter(|value| !value.is_empty())
                {
                    item["namespace"] = Value::String(namespace.to_string());
                }
                item
            }
            None => {
                json!({"id":item_id,"type":"function_call","status":status,"call_id":call_id,"name":self.tool_context.restore_name(chat_name),"arguments":arguments})
            }
        }
    }

    pub fn has_substantive_output(&self) -> bool {
        !self.content.trim().is_empty()
            || !self.reasoning.trim().is_empty()
            || !self.think_buffer.trim().is_empty()
            || self.tool_calls.values().any(|call| {
                call.added || !call.arguments.trim().is_empty() || !call.name.trim().is_empty()
            })
    }

    pub fn has_finish_reason(&self) -> bool {
        self.finish_reason.is_some()
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

fn custom_tool_input_from_chat_arguments(arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(obj)) => obj
            .get("input")
            .and_then(Value::as_str)
            .unwrap_or(arguments)
            .to_string(),
        _ => arguments.to_string(),
    }
}

fn parse_tool_arguments_object(arguments: &str) -> Value {
    if arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str::<Value>(arguments)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"query": arguments}))
}
