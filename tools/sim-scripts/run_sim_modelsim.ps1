# run_sim_modelsim.ps1 - Vivado + ModelSim simulation orchestration script
#
# 7-step workflow:
#   1. Check environment (Vivado/ModelSim paths)
#   2. Vivado: export compile order + simlib (if enabled)
#   3. Clean/create sim_work directory
#   4. vlog: compile RTL/TB
#   5. vsim: run simulation with Tcl script
#   6. Generate run_summary.json
#   7. Hand off to wave-analyzer-mcp for post-analysis
#
# Usage:
#   powershell -File run_sim_modelsim.ps1 -ConfigPath .\sim_config.yaml
#   powershell -File run_sim_modelsim.ps1 -ConfigPath .\sim_config.yaml -CleanWork
#   powershell -File run_sim_modelsim.ps1 -ConfigPath .\sim_config.yaml -SkipCompile -SkipSimulate
#
# YAML parsing: embedded minimal parser (no online modules required)

param(
    [Parameter(Mandatory=$true)]
    [string]$ConfigPath,

    [switch]$CleanWork,
    [switch]$SkipCompile,
    [switch]$SkipSimulate
)

$ErrorActionPreference = "Stop"

# --- Minimal YAML parser (handles flat + nested 1-level) ---
function Parse-Yaml {
    param([string]$Path)
    $content = Get-Content $Path -Raw
    $lines = $content -split "`n"
    $result = @{}
    $currentSection = ""
    $lastSubKey = $null

    foreach ($line in $lines) {
        $line = $line.Trim()
        if ($line -eq "" -or $line.StartsWith("#")) { continue }

        # Top-level key (no leading spaces)
        if ($line -match "^(\w+):\s*(.*)$") {
            $key = $Matches[1]
            $val = $Matches[2].Trim()
            if ($val -eq "" -or $val.StartsWith('"') -or $val.StartsWith("'")) {
                # Section or string value
                if ($val -eq "") {
                    $currentSection = $key
                    $result[$key] = @{}
                } else {
                    # Strip quotes
                    $val = $val -replace "^['""]|['""]$", ""
                    $result[$key] = $val
                }
            } else {
                $result[$key] = $val
            }
            continue
        }

        # Sub-level key (2+ leading spaces in original)
        if ($line -match "^\s+(\w+):\s*(.*)$") {
            $subKey = $Matches[1]
            $subVal = $Matches[2].Trim()
            $lastSubKey = $subKey
            if ($currentSection -ne "") {
                # Strip quotes
                $subVal = $subVal -replace "^['""]|['""]$", ""
                $result[$currentSection][$subKey] = $subVal
            }
            continue
        }

        # List item (dash)
        if ($line -match "^\s+-\s+(.*)$") {
            $listVal = $Matches[1].Trim() -replace "^['""]|['""]$", ""
            if ($currentSection -ne "" -and $null -ne $lastSubKey) {
                $listKey = "__list_$lastSubKey"
                if (-not $result[$currentSection].ContainsKey($listKey)) {
                    $result[$currentSection][$listKey] = [System.Collections.ArrayList]@()
                }
                $result[$currentSection][$listKey].Add($listVal) | Out-Null
            }
            continue
        }
    }

    return $result
}

# --- Load config ---
Write-Host "=== Step 1: Loading configuration ===" -ForegroundColor Cyan
$config = Parse-Yaml $ConfigPath

$projectRoot = $config["project_root"]
if (-not $projectRoot) { $projectRoot = "." }
$projectName = $config["project_name"]
if (-not $projectName) { $projectName = "unknown" }

# Set output directory
$outputDir = Join-Path $projectRoot "sim_output"
if (-not (Test-Path $outputDir)) {
    New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
}

Write-Host "Project: $projectName, Root: $projectRoot"

# --- Step 2: Vivado (if enabled) ---
$vivadoEnabled = $false
if ($config.ContainsKey("vivado") -and $config["vivado"]["enabled"] -eq "true") {
    $vivadoEnabled = $true

    # 查找 Vivado 可执行文件：优先 VIVADO_PATH 环境变量，其次 YAML 配置
    $vivadoBin = $null
    if ($env:VIVADO_PATH) {
        $vivadoRoot = Join-Path $env:VIVADO_PATH "Vivado"
        if (Test-Path $vivadoRoot) {
            $candidates = Get-ChildItem -Path $vivadoRoot -Filter "vivado.bat" -Recurse -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match "bin[/\\]vivado\.bat$" } |
                Sort-Object FullName -Descending
            if ($candidates) {
                $vivadoBin = $candidates[0].FullName
            }
        }
    }
    if (-not $vivadoBin -and $config["vivado"]["vivado_bin"]) {
        $vivadoBin = $config["vivado"]["vivado_bin"]
    }
    if (-not $vivadoBin) {
        Write-Host "ERROR: Vivado 未找到。请配置环境变量 VIVADO_PATH 指向 Xilinx 安装根目录（如 D:\software\Xilinx），或在 sim_config.yaml 中设置 vivado.vivado_bin" -ForegroundColor Red
        exit 1
    }

    Write-Host "=== Step 2: Vivado export ===" -ForegroundColor Cyan
    Write-Host "Vivado: $vivadoBin"

    if (-not $SkipCompile) {
        $vivadoProject = $config["vivado"]["project_file"]
        $vivadoProjectPath = Join-Path $projectRoot $vivadoProject

        # Export compile order
        $simDir = Join-Path $projectRoot "sim"
        if (-not (Test-Path $simDir)) {
            New-Item -ItemType Directory -Path $simDir -Force | Out-Null
        }

        Write-Host "Running Vivado export..."
        & $vivadoBin -mode batch -source "$PSScriptRoot\vivado_export.tcl" -project $vivadoProjectPath
        Write-Host "Vivado export complete"
    }
}

# --- Step 3: Clean/create sim_work ---
Write-Host "=== Step 3: Prepare sim_work ===" -ForegroundColor Cyan
$workLib = $config["modelsim"]["work_lib"]
if (-not $workLib) { $workLib = "sim_work" }
$workDir = Join-Path $projectRoot $workLib

if ($CleanWork -and (Test-Path $workDir)) {
    Write-Host "Cleaning $workDir..."
    Remove-Item $workDir -Recurse -Force
}

# --- Step 4: vlog compile ---
$compileOk = $true
if (-not $SkipCompile) {
    Write-Host "=== Step 4: Compile RTL/TB ===" -ForegroundColor Cyan

    $modelsimBin = $config["modelsim"]["modelsim_bin"]
    $vlogPath = Join-Path $modelsimBin "vlog.exe"

    # Build vlog command
    $vlogArgs = @("-sv", "+define+SIM")

    # Add user flags
    # Note: vlog_flags from YAML config would need list parsing
    # For simplicity, use default flags

    # Add filelist
    $filelist = $config["compile_sources"]["filelist"]
    $filelistPath = Join-Path $projectRoot $filelist

    if (Test-Path $filelistPath) {
        $vlogArgs += "-f", $filelistPath
    } else {
        Write-Host "WARNING: filelist.f not found at $filelistPath" -ForegroundColor Yellow
    }

    $vlogArgs += "-work", $workLib

    Write-Host "Running vlog..."
    $vlogOutput = & $vlogPath $vlogArgs 2>&1
    $vlogExit = $LASTEXITCODE

    if ($vlogExit -ne 0) {
        $compileOk = $false
        Write-Host "COMPILE FAILED (exit code: $vlogExit)" -ForegroundColor Red
    } else {
        Write-Host "Compile succeeded" -ForegroundColor Green
    }
}

$waveFile = $config["wave_dump"]["file"]
if (-not $waveFile) { $waveFile = "sim_output/dump.vcd" }
$waveFormat = $config["wave_dump"]["format"]
if (-not $waveFormat) { $waveFormat = "vcd" }

# --- Step 5: vsim simulate ---
$simOk = $true
$assertionFailCount = 0
$warningCount = 0
$errorCount = 0
$transcriptFile = $config["simulation"]["transcript_file"]
if (-not $transcriptFile) { $transcriptFile = "sim_output/transcript.log" }
$transcriptPath = Join-Path $projectRoot $transcriptFile

if (-not $SkipSimulate -and $compileOk) {
    Write-Host "=== Step 5: Run simulation ===" -ForegroundColor Cyan

    $modelsimBin = $config["modelsim"]["modelsim_bin"]
    $vsimPath = Join-Path $modelsimBin "vsim.exe"

    $topModule = $config["modelsim"]["top_module"]
    if (-not $topModule) { $topModule = "tb_top" }

    # --- Generate config-driven Tcl script ---

    $simMode = $config["simulation"]["mode"]
    if (-not $simMode) { $simMode = "run_all" }
    $simRunTime = $config["simulation"]["run_time"]

    # Extract recursive scopes from config (list or single string)
    $recursiveScopes = @()
    if ($config["wave_dump"].ContainsKey("__list_recursive_scopes")) {
        $recursiveScopes = @($config["wave_dump"]["__list_recursive_scopes"])
    } elseif ($config["wave_dump"]["recursive_scopes"]) {
        $recursiveScopes = @($config["wave_dump"]["recursive_scopes"])
    }
    if ($recursiveScopes.Count -eq 0) {
        $recursiveScopes = @("/${topModule}/*")
    }

    # Extract critical signals from config
    $criticalSignals = @()
    if ($config.ContainsKey("wave_dump") -and $config["wave_dump"].ContainsKey("__list_critical_signals")) {
        $criticalSignals = @($config["wave_dump"]["__list_critical_signals"])
    } elseif ($config.ContainsKey("wave_dump") -and $config["wave_dump"]["critical_signals"]) {
        $criticalSignals = @($config["wave_dump"]["critical_signals"])
    }

    # Generate Tcl script content
    $tclContent = @"
# Auto-generated ModelSim Tcl script (from sim_config.yaml)
# DO NOT EDIT MANUALLY - regenerate by running run_sim_modelsim.ps1

# --- Transcript ---
transcript file $transcriptFile

# --- Simulation ---
vsim -assertdebug ${workLib}.${topModule}

# --- Waveform dump ---
"@

    foreach ($scope in $recursiveScopes) {
        $tclContent += "`nlog -recursive $scope"
    }

    foreach ($sig in $criticalSignals) {
        $tclContent += "`nadd wave $sig"
    }

    # Run command
    if ($simMode -eq "run_all") {
        $tclContent += "`n`n# --- Run ---`nrun -all"
    } elseif ($simRunTime) {
        $tclContent += "`n`n# --- Run ---`nrun $simRunTime"
    } else {
        $tclContent += "`n`n# --- Run ---`nrun -all"
    }

    # Wave export
    if ($waveFormat -eq "vcd") {
        $tclContent += "`n`n# --- VCD export ---`nvcd file $waveFile"
        foreach ($scope in $recursiveScopes) {
            $tclContent += "`nvcd add -recursive $scope"
        }
        $tclContent += "`nvcd flush"
    } elseif ($waveFormat -eq "wlf") {
        $tclContent += "`n`n# --- WLF saved automatically by vsim ---"
    }

    $tclContent += "`n`n# --- Done ---`nquit -f"

    # Write generated Tcl script
    $tclPath = Join-Path $projectRoot "sim_output\modelsim_run_generated.tcl"
    Set-Content -Path $tclPath -Value $tclContent -Encoding UTF8
    Write-Host "Generated Tcl script: $tclPath"

    # Build vsim command using generated Tcl
    $vsimArgs = @("-c", "-t", "1ps", "-assertdebug", "-do", "do $tclPath", "${workLib}.${topModule}")

    Write-Host "Running vsim..."
    & $vsimPath $vsimArgs 2>&1 | Out-Null
    $vsimExit = $LASTEXITCODE

    # Parse transcript for assertion statistics
    if (Test-Path $transcriptPath) {
        $transcriptContent = Get-Content $transcriptPath -Raw

        # Count assertion failures (vsim-10142 pattern)
        $assertionFailCount = ([regex]::Matches($transcriptContent, "vsim-10142")).Count
        $errorCount = ([regex]::Matches($transcriptContent, "(Error|Failure)")).Count
        $warningCount = ([regex]::Matches($transcriptContent, "Warning")).Count

        if ($vsimExit -ne 0) {
            $simOk = $false
            Write-Host "SIMULATION FAILED" -ForegroundColor Red
        } else {
            Write-Host "Simulation completed" -ForegroundColor Green
        }
    }
}

# --- Step 6: Generate run_summary.json ---
Write-Host "=== Step 6: Generate run_summary.json ===" -ForegroundColor Cyan

# Determine overall status
$status = "passed"
if (-not $compileOk) {
    $status = "compile_failed"
} elseif (-not $simOk) {
    # Distinguish elaboration failure (vsim cannot start) from simulation failure
    if (-not (Test-Path $transcriptPath)) {
        $status = "elab_failed"
    } else {
        $status = "simulation_failed"
    }
} elseif ($assertionFailCount -gt 0) {
    $status = "assertion_failed"
}

$summary = @{
    status = $status
    project_name = $projectName
    top_module = $config["modelsim"]["top_module"]
    compile_ok = $compileOk
    elab_ok = $compileOk
    simulation_ok = $simOk
    assertion_fail_count = $assertionFailCount
    warning_count = $warningCount
    error_count = $errorCount
    wave_file = $waveFile
    wave_format = $waveFormat
    transcript_file = $transcriptFile
    simulator = "modelsim"
    finished_at = (Get-Date -Format "yyyy-MM-ddTHH:mm:ss")
}

$summaryPath = Join-Path $projectRoot ($config["simulation"]["summary_file"])
if (-not $summaryPath) { $summaryPath = Join-Path $projectRoot "sim_output/run_summary.json" }

$summaryJson = $summary | ConvertTo-Json -Depth 3
Set-Content -Path $summaryPath -Value $summaryJson -Encoding UTF8

Write-Host "Status: $status" -ForegroundColor $(if ($status -eq "passed") {"Green"} elseif ($status -eq "assertion_failed") {"Yellow"} else {"Red"})
Write-Host "Summary saved to $summaryPath"

# --- Step 7: Post-analysis hint ---
if ($status -eq "assertion_failed") {
    Write-Host "=== Step 7: Post-analysis ===" -ForegroundColor Cyan
    Write-Host "Use wave-analyzer-cli for root cause analysis:"
    Write-Host "  wave-analyzer-cli open_waveform $waveFile -- \"
    Write-Host "    load_assertion_log $transcriptFile --severity-filter Error,Failure -- \"
    Write-Host "    load_dependencies specs/deps.yaml -- \"
    Write-Host "    batch_trace_root_cause wave deps assertions"
}

Write-Host "=== Done ===" -ForegroundColor Cyan