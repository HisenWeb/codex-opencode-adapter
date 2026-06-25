[CmdletBinding()]
param(
    [string]$ApiKey,
    [string]$ListenHost = "127.0.0.1",
    [int]$Port = 4010,
    [string]$UpstreamBase = "https://opencode.ai/zen/go/v1",
    [switch]$PrintOnly
)

$ErrorActionPreference = "Stop"

if ($PrintOnly) {
    @"
[model_providers.opencode_go_adapter]
name = "OpenCode Go Adapter"
base_url = "http://127.0.0.1:4010/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
stream_idle_timeout_ms = 120000

[model_providers.opencode_go_adapter.auth]
command = "codex-opencode-adapter"
args = ["auth", "print-local-token"]
timeout_ms = 1000
"@
    exit 0
}

Write-Warning "scripts/install-user-provider.ps1 is a legacy wrapper. Use `codex-opencode-adapter init` directly when possible."

$command = Get-Command "codex-opencode-adapter" -ErrorAction SilentlyContinue
if (-not $command) {
    throw "codex-opencode-adapter is not installed or not on PATH. Install it first, then run `codex-opencode-adapter init`."
}

$arguments = @("init", "--host", $ListenHost, "--port", $Port, "--upstream-base", $UpstreamBase)
if ($ApiKey) {
    $arguments += @("--api-key", $ApiKey)
}

& $command.Source @arguments
exit $LASTEXITCODE
