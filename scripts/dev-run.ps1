[CmdletBinding()]
param(
    [string]$ApiKey = "",
    [string]$BaseUrl = "https://opencode.ai/zen/go/v1",
    [string]$ListenHost = "127.0.0.1",
    [int]$Port = 4010,
    [string]$LocalToken = "codex-opencode-local",
    [string]$StateDb = ".codex-opencode/state.sqlite",
    [int]$MaxConcurrency = 8,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($ApiKey)) {
    $ApiKey = $env:OPENCODE_GO_API_KEY
}

if ([string]::IsNullOrWhiteSpace($ApiKey)) {
    throw "OPENCODE_GO_API_KEY is required. Pass -ApiKey or set the environment variable first."
}

if (-not [System.IO.Path]::IsPathRooted($StateDb)) {
    $StateDb = Join-Path $repoRoot $StateDb
}

$existing = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($existing) {
    Write-Host "Stopping existing process on port $Port (PID $($existing.OwningProcess))"
    Stop-Process -Id $existing.OwningProcess -Force
    Start-Sleep -Milliseconds 500
}

$env:OPENCODE_GO_API_KEY = $ApiKey
$env:OPENCODE_GO_BASE_URL = $BaseUrl
$env:CODEX_OPENCODE_HOST = $ListenHost
$env:CODEX_OPENCODE_PORT = "$Port"
$env:CODEX_OPENCODE_LOCAL_TOKEN = $LocalToken
$env:CODEX_OPENCODE_STATE_DB = $StateDb
$env:CODEX_OPENCODE_MAX_CONCURRENCY = "$MaxConcurrency"

Write-Host "Starting adapter from repo:"
Write-Host " - repo root: $repoRoot"
Write-Host " - base URL:  $BaseUrl"
Write-Host " - listen:    http://${ListenHost}:$Port"
Write-Host " - state DB:  $StateDb"
Write-Host " - concurrency: $MaxConcurrency"

Push-Location $repoRoot
try {
    if ($Release) {
        cargo run --release
    } else {
        cargo run
    }
} finally {
    Pop-Location
}
