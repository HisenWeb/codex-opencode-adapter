[CmdletBinding()]
param(
    [string]$ListenHost = "127.0.0.1",
    [int]$Port = 4010,
    [string]$LocalToken = "codex-opencode-local"
)

$ErrorActionPreference = "Stop"

$baseUrl = "http://{0}:{1}" -f $ListenHost, $Port
$headers = @{ Authorization = "Bearer $LocalToken" }

Write-Host "Checking $baseUrl/health"
$health = Invoke-RestMethod "$baseUrl/health"
if ($health.status -ne "ok") {
    throw "Health check did not return status=ok."
}

Write-Host "Checking $baseUrl/v1/models"
$models = Invoke-RestMethod "$baseUrl/v1/models" -Headers $headers
$modelIds = @($models.data | ForEach-Object { $_.id })

if ($modelIds.Count -eq 0) {
    throw "Model list is empty."
}

Write-Host "Adapter is reachable and authenticated."
Write-Host "Discovered models:"
$modelIds | ForEach-Object { Write-Host " - $_" }
