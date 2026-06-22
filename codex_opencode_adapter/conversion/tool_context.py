from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any

from .ids import compact_json

JSON = dict[str, Any]
_SAFE_TOOL_NAME = re.compile(r"[^a-zA-Z0-9_-]")
CHAT_TOOL_NAME_MAX_LEN = 64
CUSTOM_TOOL_INPUT_FIELD = "input"


@dataclass
class ToolSpec:
    kind: str
    name: str
    chat_name: str
    namespace: str | None = None


@dataclass
class ToolContext:
    chat_tools: list[JSON] = field(default_factory=list)
    reverse_names: dict[str, str] = field(default_factory=dict)
    specs_by_chat_name: dict[str, ToolSpec] = field(default_factory=dict)
    namespace_lookup: dict[tuple[str, str], str] = field(default_factory=dict)
    _used: set[str] = field(default_factory=set)

    def is_custom_tool_chat_name(self, name: str) -> bool:
        spec = self.specs_by_chat_name.get(name)
        return bool(spec and spec.kind in {"custom", "tool_search"})

    def restore_name(self, name: str) -> str:
        return self.reverse_names.get(name, name)

    def add_response_tool(self, item: Any) -> None:
        if isinstance(item, str):
            self._add_custom({"type": "custom", "name": item})
            return
        if not isinstance(item, dict):
            return
        kind = str(item.get("type") or "function")
        if kind == "namespace":
            self._add_namespace(item)
        elif kind in {"custom", "tool_search"}:
            self._add_custom(item, kind=kind)
        else:
            source = _tool_source(item)
            if isinstance(source, dict):
                self._add_function(source)

    def _safe_name(self, original: str) -> str:
        base = _SAFE_TOOL_NAME.sub("_", original.strip())[:CHAT_TOOL_NAME_MAX_LEN] or "tool"
        candidate = base
        suffix = 2
        while candidate in self._used:
            tail = f"_{suffix}"
            candidate = f"{base[: CHAT_TOOL_NAME_MAX_LEN - len(tail)]}{tail}"
            suffix += 1
        self._used.add(candidate)
        return candidate

    def _add_chat_tool(self, original: str, spec: ToolSpec, parameters: JSON, description: str) -> None:
        if not original:
            return
        self.reverse_names[spec.chat_name] = original
        self.specs_by_chat_name[spec.chat_name] = spec
        if spec.namespace:
            self.namespace_lookup[(spec.namespace, spec.name)] = spec.chat_name
        self.chat_tools.append(
            {
                "type": "function",
                "function": {
                    "name": spec.chat_name,
                    "description": description,
                    "parameters": parameters,
                },
            }
        )

    def _add_function(self, source: JSON, *, original_name: str | None = None, namespace: str | None = None) -> None:
        original = str(original_name or source.get("name") or "").strip()
        if not original:
            return
        chat_name = self._safe_name(original)
        parameters = source.get("parameters")
        if not isinstance(parameters, dict):
            parameters = {"type": "object", "properties": {}}
        spec = ToolSpec(
            kind="namespace" if namespace else "function",
            name=str(source.get("name") or original),
            chat_name=chat_name,
            namespace=namespace,
        )
        self._add_chat_tool(original, spec, parameters, str(source.get("description") or ""))

    def _add_custom(self, source: JSON, *, kind: str = "custom") -> None:
        original = str(source.get("name") or "").strip()
        if not original:
            return
        description = str(source.get("description") or "")
        preserved = compact_json(source)
        desc = (
            f"{description}\n\nOriginal Responses custom tool definition:\n{preserved}"
            if description
            else f"Original Responses custom tool definition:\n{preserved}"
        )
        chat_name = self._safe_name(original)
        spec = ToolSpec(kind=kind, name=original, chat_name=chat_name)
        self._add_chat_tool(
            original,
            spec,
            {
                "type": "object",
                "properties": {
                    CUSTOM_TOOL_INPUT_FIELD: {
                        "type": "string",
                        "description": "Raw string input for the original custom tool.",
                    }
                },
                "required": [CUSTOM_TOOL_INPUT_FIELD],
            },
            desc,
        )

    def _add_namespace(self, source: JSON) -> None:
        namespace = str(source.get("name") or "").strip()
        children = source.get("tools") or source.get("children") or []
        if not namespace or not isinstance(children, list):
            return
        for child in children:
            if not isinstance(child, dict):
                continue
            child_source = _tool_source(child)
            if not isinstance(child_source, dict):
                continue
            child_name = str(child_source.get("name") or "").strip()
            if child_name:
                self._add_function(child_source, original_name=f"{namespace}__{child_name}", namespace=namespace)


def _tool_source(item: JSON) -> JSON | None:
    nested = item.get("function")
    return nested if isinstance(nested, dict) else item


def build_tool_context(tools: Any) -> ToolContext:
    context = ToolContext()
    if isinstance(tools, list):
        for item in tools:
            context.add_response_tool(item)
    return context


def convert_tools(tools: Any) -> tuple[list[JSON], dict[str, str]]:
    context = build_tool_context(tools)
    return context.chat_tools, context.reverse_names


def restore_tool_name(name: str, reverse: dict[str, str]) -> str:
    return reverse.get(name, name)


def convert_tool_choice(tool_choice: Any, context: ToolContext) -> Any:
    if tool_choice is None:
        return None
    if isinstance(tool_choice, str):
        return tool_choice
    if not isinstance(tool_choice, dict):
        return tool_choice
    kind = tool_choice.get("type")
    if kind in {"auto", "none", "required"}:
        return kind
    if kind in {"function", "tool"}:
        name = str(tool_choice.get("name") or "")
        chat_name = next((safe for safe, original in context.reverse_names.items() if original == name), name)
        return {"type": "function", "function": {"name": chat_name}}
    return tool_choice
