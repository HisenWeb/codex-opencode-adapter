## Subagent Policy

- Prefer OSS subagents over GPT/default subagents unless the user explicitly asks otherwise.
- For review, investigation, comparison, and codebase scanning tasks, prefer spawning OSS subagents asynchronously.
- Do not immediately wait on a spawned subagent unless:
  - the user explicitly asks for immediate results, or
  - the next step depends on that subagent’s output.
- When possible, continue local analysis or other parallel work before collecting subagent results.

## Subagent Output Limits

- Ask subagents to return concise, decision-useful summaries rather than long transcripts.
- Default to returning:
  1. a short conclusion,
  2. up to 5 findings,
  3. file paths and line numbers for each finding,
  4. a brief recommendation for each actionable issue.
- Do not return full files, large code blocks, or full logs unless explicitly requested.
- Quote only the minimum snippet needed to support a finding.
- If no actionable issue is found, say so clearly and stop.