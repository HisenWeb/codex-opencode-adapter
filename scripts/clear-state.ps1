[CmdletBinding()]
param(
    [string]$StateDb
)

$repoRoot = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($StateDb)) {
    if (-not [string]::IsNullOrWhiteSpace($env:CODEX_OPENCODE_STATE_DB)) {
        $StateDb = $env:CODEX_OPENCODE_STATE_DB
    } else {
        $StateDb = ".codex-opencode/state.sqlite"
    }
}

if (-not [System.IO.Path]::IsPathRooted($StateDb)) {
    $StateDb = Join-Path $repoRoot $StateDb
}

$resolvedPath = [System.IO.Path]::GetFullPath($StateDb)

if (-not (Test-Path -LiteralPath $resolvedPath)) {
    Write-Host "State DB not found: $resolvedPath"
    exit 0
}

Remove-Item -LiteralPath $resolvedPath -Force
Write-Host "Deleted state DB: $resolvedPath"
