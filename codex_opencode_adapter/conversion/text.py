from __future__ import annotations

from typing import Any

from .ids import compact_json


def as_text(value: Any) -> str:
    """Best-effort text extraction for Responses and Chat content shapes."""
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, (int, float, bool)):
        return str(value)
    if isinstance(value, list):
        parts: list[str] = []
        for item in value:
            if isinstance(item, dict):
                kind = item.get("type")
                if kind in {"input_text", "output_text", "text", "refusal"}:
                    parts.append(str(item.get("text") or item.get("refusal") or ""))
                elif kind in {"tool_result", "function_call_output"}:
                    parts.append(as_text(item.get("content", item.get("output", ""))))
                elif "content" in item:
                    parts.append(as_text(item["content"]))
                elif "text" in item:
                    parts.append(str(item["text"]))
                else:
                    parts.append(compact_json(item))
            else:
                parts.append(as_text(item))
        return "\n".join(part for part in parts if part)
    if isinstance(value, dict):
        if "text" in value:
            return str(value["text"])
        if "content" in value:
            return as_text(value["content"])
        if "output" in value:
            return as_text(value["output"])
        return compact_json(value)
    return str(value)


def arguments_text(value: Any) -> str:
    return value if isinstance(value, str) else compact_json(value)


def reasoning_text(value: dict[str, Any]) -> str:
    for key in ("reasoning_content", "reasoning", "thinking"):
        if value.get(key):
            return as_text(value[key])
    return ""
