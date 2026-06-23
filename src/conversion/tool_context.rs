use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const CHAT_TOOL_NAME_MAX_LEN: usize = 64;
const CUSTOM_TOOL_INPUT_FIELD: &str = "input";
const TOOL_SEARCH_NAME: &str = "tool_search";

pub fn custom_tool_input_field() -> &'static str {
    CUSTOM_TOOL_INPUT_FIELD
}

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
    namespace_name_to_chat_name: HashMap<(String, String), String>,
    used: HashSet<String>,
}

impl ToolContext {
    pub fn build(tools: Option<&Value>) -> Self {
        Self::build_with_input(tools, None)
    }

    pub fn build_with_input(tools: Option<&Value>, input: Option<&Value>) -> Self {
        let mut context = Self::default();
        if let Some(Value::Array(items)) = tools {
            for item in items {
                context.add_response_tool(item);
            }
        }
        if let Some(input) = input {
            context.collect_tool_search_output_tools(input);
        }
        context
    }

    pub fn restore_name(&self, name: &str) -> String {
        self.reverse_names.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    pub fn is_custom_tool_chat_name(&self, name: &str) -> bool {
        self.specs_by_chat_name
            .get(name)
            .map(|spec| matches!(spec.kind, ToolKind::Custom))
            .unwrap_or(false)
    }

    pub fn is_tool_search_chat_name(&self, name: &str) -> bool {
        self.specs_by_chat_name
            .get(name)
            .map(|spec| matches!(spec.kind, ToolKind::ToolSearch))
            .unwrap_or(false)
    }

    pub fn lookup_spec(&self, chat_name: &str) -> Option<&ToolSpec> {
        self.specs_by_chat_name.get(chat_name)
    }

    pub fn chat_name_for_response_function(&self, name: &str, namespace: Option<&str>) -> String {
        if let Some(namespace) = namespace.filter(|value| !value.is_empty()) {
            if let Some(chat_name) = self.namespace_name_to_chat_name.get(&(namespace.to_string(), name.to_string())) {
                return chat_name.clone();
            }
            return format!("{namespace}__{name}").chars().take(CHAT_TOOL_NAME_MAX_LEN).collect();
        }
        self.reverse_names
            .iter()
            .find_map(|(safe, original)| (original == name).then_some(safe.clone()))
            .unwrap_or_else(|| name.to_string())
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
            "tool_search" => self.add_tool_search(item),
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

    fn add_tool_search(&mut self, item: &Value) {
        let original = TOOL_SEARCH_NAME;
        let chat_name = self.safe_name(original);
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Search for tools available to the client.")
            .to_string();
        let spec = ToolSpec {
            kind: ToolKind::ToolSearch,
            name: original.to_string(),
            chat_name: chat_name.clone(),
            namespace: None,
        };
        self.add_chat_tool(original, spec, json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query for tools."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tools to return."
                }
            },
            "required": ["query"]
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
            namespace: namespace.clone(),
        };
        if let Some(namespace) = namespace {
            self.namespace_name_to_chat_name.insert((namespace, spec.name.clone()), chat_name.clone());
        }
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

    fn collect_tool_search_output_tools(&mut self, value: &Value) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.collect_tool_search_output_tools(item);
                }
            }
            Value::Object(obj) => {
                if obj.get("type").and_then(Value::as_str) == Some("tool_search_output") {
                    if let Some(tools) = obj.get("tools").and_then(Value::as_array) {
                        for tool in tools {
                            self.add_response_tool(tool);
                        }
                    }
                }
                for value in obj.values() {
                    self.collect_tool_search_output_tools(value);
                }
            }
            _ => {}
        }
    }

    fn safe_name(&mut self, original: &str) -> String {
        let mut base = original
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .take(CHAT_TOOL_NAME_MAX_LEN)
            .collect::<String>();
        if base.is_empty() { base = "tool".to_string(); }
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self.used.contains(&candidate) {
            let tail = format!("_{suffix}");
            let keep = CHAT_TOOL_NAME_MAX_LEN.saturating_sub(tail.len());
            let head = base.chars().take(keep).collect::<String>();
            candidate = format!("{head}{tail}");
            suffix += 1;
        }
        self.used.insert(candidate.clone());
        candidate
    }
}

fn tool_source(item: &Value) -> Option<&Value> {
    item.get("function").filter(|v| v.is_object()).or(Some(item))
}
