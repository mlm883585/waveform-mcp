$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir
$depsExtractorDir = Join-Path $repoRoot "tools\deps-extractor"
$sidecarExe = Join-Path $depsExtractorDir "dist\wave-analyzer-deps-extractor.exe"

Push-Location $depsExtractorDir
try {
    python -m pip install pyinstaller -r requirements.txt
    python build_sidecar.py
} finally {
    Pop-Location
}

if (-not (Test-Path $sidecarExe)) {
    throw "Missing sidecar: $sidecarExe"
}

Push-Location $repoRoot
try {
    cargo build --release --bin wave-analyzer-cli
} finally {
    Pop-Location
}
