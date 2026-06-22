use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub type Json = Value;
const CHAT_TOOL_NAME_MAX_LEN: usize = 64;
const CUSTOM_TOOL_INPUT_FIELD: &str = "input";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolKind {
    Function,
    Namespace,
    Custom,
    ToolSearch,
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub kind: ToolKind,
    pub name: String,
    pub chat_name: String,
    pub namespace: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ToolContext {
    pub chat_tools: Vec<Value>,
    pub reverse_names: HashMap<String, String>,
    specs_by_chat_name: HashMap<String, ToolSpec>,
    used: HashSet<String>,
}

impl ToolContext {
    pub fn build(tools: Option<&Value>) -> Self {
        let mut context = Self::default();
        if let Some(Value::Array(items)) = tools {
            for item in items {
                context.add_response_tool(item);
            }
        }
        context
    }

    pub fn restore_name(&self, name: &str) -> String {
        self.reverse_names.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    pub fn is_custom_tool_chat_name(&self, name: &str) -> bool {
        self.specs_by_chat_name
            .get(name)
            .map(|spec| matches!(spec.kind, ToolKind::Custom | ToolKind::ToolSearch))
            .unwrap_or(false)
    }

    fn add_response_tool(&mut self, item: &Value) {
        if let Some(name) = item.as_str() {
            self.add_custom(&json!({"type":"custom","name":name}), ToolKind::Custom);
            return;
        }
        let Some(obj) = item.as_object() else { return; };
        match obj.get("type").and_then(Value::as_str).unwrap_or("function") {
            "namespace" => self.add_namespace(item),
            "custom" => self.add_custom(item, ToolKind::Custom),
            "tool_search" => self.add_custom(item, ToolKind::ToolSearch),
            _ => self.add_function(tool_source(item).unwrap_or(item), None, None),
        }
    }

    fn add_namespace(&mut self, item: &Value) {
        let namespace = item.get("name").and_then(Value::as_str).unwrap_or("").trim();
        let Some(children) = item.get("tools").or_else(|| item.get("children")).and_then(Value::as_array) else { return; };
        if namespace.is_empty() { return; }
        for child in children {
            let source = tool_source(child).unwrap_or(child);
            if let Some(child_name) = source.get("name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                self.add_function(source, Some(format!("{namespace}__{child_name}")), Some(namespace.to_string()));
            }
        }
    }

    fn add_custom(&mut self, item: &Value, kind: ToolKind) {
        let original = item.get("name").and_then(Value::as_str).unwrap_or("").trim();
        if original.is_empty() { return; }
        let chat_name = self.safe_name(original);
        let description = item.get("description").and_then(Value::as_str).unwrap_or("");
        let preserved = serde_json::to_string(item).unwrap_or_default();
        let description = if description.is_empty() {
            format!("Original Responses custom tool definition:\n{preserved}")
        } else {
            format!("{description}\n\nOriginal Responses custom tool definition:\n{preserved}")
        };
        let spec = ToolSpec { kind, name: original.to_string(), chat_name: chat_name.clone(), namespace: None };
        self.add_chat_tool(original, spec, json!({
            "type": "object",
            "properties": {
                CUSTOM_TOOL_INPUT_FIELD: {
                    "type": "string",
                    "description": "Raw string input for the original custom tool."
                }
            },
            "required": [CUSTOM_TOOL_INPUT_FIELD]
        }), description);
    }

    fn add_function(&mut self, source: &Value, original_name: Option<String>, namespace: Option<String>) {
        let original = original_name.unwrap_or_else(|| source.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string());
        if original.is_empty() { return; }
        let chat_name = self.safe_name(&original);
        let parameters = source.get("parameters").cloned().filter(Value::is_object).unwrap_or_else(|| json!({"type":"object","properties":{}}));
        let spec = ToolSpec {
            kind: if namespace.is_some() { ToolKind::Namespace } else { ToolKind::Function },
            name: source.get("name").and_then(Value::as_str).unwrap_or(&original).to_string(),
            chat_name: chat_name.clone(),
            namespace,
        };
        self.add_chat_tool(original.as_str(), spec, parameters, source.get("description").and_then(Value::as_str).unwrap_or("").to_string());
    }

    fn add_chat_tool(&mut self, original: &str, spec: ToolSpec, parameters: Value, description: String) {
        self.reverse_names.insert(spec.chat_name.clone(), original.to_string());
        self.specs_by_chat_name.insert(spec.chat_name.clone(), spec.clone());
        self.chat_tools.push(json!({
            "type": "function",
            "function": {
                "name": spec.chat_name,
                "description": description,
                "parameters": parameters,
            }
        }));
    }

    fn safe_name(&mut self, original: &str) -> String {
        let mut base = original
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect::<String>();
        if base.len() > CHAT_TOOL_NAME_MAX_LEN { base.truncate(CHAT_TOOL_NAME_MAX_LEN); }
        if base.is_empty() { base = "tool".to_string(); }
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self.used.contains(&candidate) {
            let tail = format!("_{suffix}");
            let keep = CHAT_TOOL_NAME_MAX_LEN.saturating_sub(tail.len());
            candidate = format!("{}{}", &base[..base.len().min(keep)], tail);
            suffix += 1;
        }
        self.used.insert(candidate.clone());
        candidate
    }
}

fn tool_source(item: &Value) -> Option<&Value> {
    item.get("function").filter(|v| v.is_object()).or(Some(item))
}
