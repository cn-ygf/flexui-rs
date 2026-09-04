$ErrorActionPreference = "Stop"

$RepoDir = Split-Path -Parent $PSScriptRoot
Push-Location $RepoDir
try {
    cargo run -p flexui --bin window_lifecycle_smoke
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
finally {
    Pop-Location
}
