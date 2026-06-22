from codex_opencode_adapter.conversion.responses_to_chat import build_chat_payload
from codex_opencode_adapter.conversion.stream_chat_to_responses import StreamAssembler
from codex_opencode_adapter.conversion.tool_context import build_tool_context


def test_conversion_package_exports_tool_context():
    context = build_tool_context(
        [
            {
                "type": "namespace",
                "name": "mcp",
                "tools": [
                    {
                        "type": "function",
                        "name": "read.file",
                        "description": "Read file",
                        "parameters": {"type": "object", "properties": {}},
                    }
                ],
            },
            {"type": "custom", "name": "shell.exec"},
        ]
    )
    names = [tool["function"]["name"] for tool in context.chat_tools]
    assert names == ["mcp__read_file", "shell_exec"]
    assert context.restore_name("mcp__read_file") == "mcp__read.file"
    assert context.is_custom_tool_chat_name("shell_exec")


def test_request_transform_keeps_old_protocol_facade_contract():
    payload, messages, reverse = build_chat_payload(
        {
            "model": "opencode-go/deepseek-v4-pro",
            "instructions": "System.",
            "input": [{"type": "message", "role": "developer", "content": "Dev."}, "Hi"],
            "tools": [{"type": "function", "name": "mcp.read", "parameters": {"type": "object"}}],
            "tool_choice": {"type": "function", "name": "mcp.read"},
            "stream": True,
        },
        model_upstream="deepseek-v4-pro",
        previous=None,
        reasoning_parameter={},
    )
    assert messages[0] == {"role": "system", "content": "System.\n\nDev."}
    assert payload["stream_options"] == {"include_usage": True}
    assert payload["tool_choice"] == {"type": "function", "function": {"name": "mcp_read"}}
    assert reverse == {"mcp_read": "mcp.read"}


def test_stream_reasoning_lifecycle_is_emitted():
    events = []
    assembler = StreamAssembler(
        body={"model": "opencode-go/deepseek-v4-pro", "input": "x"},
        model_alias="opencode-go/deepseek-v4-pro",
        model_upstream="deepseek-v4-pro",
        base_messages=[{"role": "user", "content": "x"}],
        reverse_names={},
        state_put=lambda _: None,
        emit=lambda event, data: events.append((event, data)),
    )
    assembler.start()
    assembler.accept({"choices": [{"delta": {"reasoning_content": "think"}}]})
    assembler.accept({"choices": [{"delta": {"content": "answer"}, "finish_reason": "stop"}]})
    response = assembler.finalize()
    names = [name for name, _ in events]
    assert "response.reasoning_summary_text.delta" in names
    assert "response.reasoning_summary_text.done" in names
    assert "response.output_text.delta" in names
    assert response["output"][0]["type"] == "reasoning"
    assert response["output"][1]["type"] == "message"
