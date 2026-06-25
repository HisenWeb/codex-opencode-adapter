param(
    [string]$BaseUrl = "https://opencode.ai/zen/go/v1",
    [string]$TextModel = "opencode-go/deepseek-v4-flash",
    [string]$VisionModel = "opencode-go/mimo-v2.5",
    [string]$ApiKey = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ApiKey)) {
    $ApiKey = $env:OPENCODE_GO_API_KEY
}

if ([string]::IsNullOrWhiteSpace($ApiKey)) {
    throw "OPENCODE_GO_API_KEY is required. Pass -ApiKey or set the environment variable first."
}

$env:OPENCODE_GO_API_KEY = $ApiKey
$env:OPENCODE_GO_BASE_URL = $BaseUrl
$env:OPENCODE_GO_REAL_TEXT_MODEL = $TextModel
$env:OPENCODE_GO_REAL_VISION_MODEL = $VisionModel

Write-Host "Running real smoke suite against $BaseUrl"
Write-Host "Text model: $TextModel"
Write-Host "Vision model: $VisionModel"

cargo test --test e2e_real_smoke test_e2e_real_validation_suite -- --ignored --nocapture
