//! deps_extractor -- Rust-side orchestration for the deps-extractor Python pipeline
//!
//! Calls `extract_deps_pyverilog.py` (or Vivado Tcl) and `deps_converter.py`
//! as subprocesses to generate deps.yaml from RTL source files.

include!(concat!(env!("OUT_DIR"), "/embedded_deps_sidecar.rs"));

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{WaveAnalyzerError, WaveResult};

/// Result of a deps-extractor run
pub struct DepsExtractorResult {
    /// Path to the generated deps.yaml file
    pub deps_yaml_path: String,
    /// Engine used: "pyverilog" or "vivado"
    pub engine: String,
    /// Number of dependencies (nodes) in the generated graph
    pub node_count: Option<usize>,
    /// Stdout output from the extraction process
    pub stdout: String,
}

/// Find the deps-extractor directory
///
/// Priority:
/// 1. Explicitly provided path
/// 2. Environment variable DEPS_EXTRACTOR_PATH
/// 3. Relative to this crate's location (wave-analyzer-mcp/tools/deps-extractor)
fn find_deps_extractor_dir(explicit_path: Option<&str>) -> Option<PathBuf> {
    // 1. Explicitly provided
    if let Some(path) = explicit_path {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Environment variable
    if let Some(env_path) = std::env::var_os("DEPS_EXTRACTOR_PATH") {
        let p = PathBuf::from(&env_path);
        if p.exists() {
            return Some(p);
        }
    }

    // 3. Relative to current executable or working directory
    // Try common locations
    let candidates = [
        "wave-analyzer-mcp/tools/deps-extractor",
        "tools/deps-extractor",
    ];

    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        for candidate in &candidates {
            let p = exe_dir.join(candidate);
            if p.exists() {
                return Some(p);
            }
        }

        if let Some(parent) = exe_dir.parent() {
            for candidate in &candidates {
                let p = parent.join(candidate);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    for candidate in &candidates {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }

    // 4. Try relative to the current working directory
    if let Ok(cwd) = std::env::current_dir() {
        for candidate in &candidates {
            let p = cwd.join(candidate);
            if p.exists() {
                return Some(p);
            }
        }
        // Also try parent directories (monorepo structure)
        if let Some(parent) = cwd.parent() {
            for candidate in &candidates {
                let p = parent.join(candidate);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }

    None
}

fn resolve_output_paths(
    rtl_path: &Path,
    output_path: Option<&str>,
) -> WaveResult<(PathBuf, PathBuf)> {
    let output_file = match output_path {
        Some(out) => {
            let candidate = PathBuf::from(out);
            let treat_as_dir =
                candidate.exists() && candidate.is_dir() || candidate.extension().is_none();
            if treat_as_dir {
                candidate.join("deps.yaml")
            } else {
                candidate
            }
        }
        None => {
            let base_dir = rtl_path.parent().unwrap_or_else(|| Path::new("."));
            base_dir.join("deps.yaml")
        }
    };

    let output_dir = output_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&output_dir).map_err(|e| WaveAnalyzerError::FileError {
        path: output_dir.display().to_string(),
        message: e.to_string(),
    })?;

    let deps_raw_json = output_dir.join("deps_raw.json");
    Ok((deps_raw_json, output_file))
}

fn find_sidecar_exe(extractor_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        extractor_dir.join("wave-analyzer-deps-extractor.exe"),
        extractor_dir.join("wave-analyzer-deps-extractor"),
        extractor_dir
            .join("dist")
            .join("wave-analyzer-deps-extractor.exe"),
        extractor_dir
            .join("dist")
            .join("wave-analyzer-deps-extractor"),
        extractor_dir
            .join("bin")
            .join("wave-analyzer-deps-extractor.exe"),
        extractor_dir
            .join("bin")
            .join("wave-analyzer-deps-extractor"),
    ];

    candidates.into_iter().find(|p| p.exists())
}

fn materialize_embedded_sidecar() -> WaveResult<Option<PathBuf>> {
    let bytes = match EMBEDDED_DEPS_SIDECAR {
        Some(bytes) => bytes,
        None => return Ok(None),
    };

    let dir = std::env::temp_dir().join("wave-analyzer-mcp");
    std::fs::create_dir_all(&dir).map_err(|e| WaveAnalyzerError::FileError {
        path: dir.display().to_string(),
        message: e.to_string(),
    })?;
    let sidecar = dir.join("wave-analyzer-deps-extractor.exe");

    let should_write = match std::fs::read(&sidecar) {
        Ok(existing) => existing != bytes,
        Err(_) => true,
    };

    if should_write {
        std::fs::write(&sidecar, bytes).map_err(|e| WaveAnalyzerError::FileError {
            path: sidecar.display().to_string(),
            message: e.to_string(),
        })?;
    }

    Ok(Some(sidecar))
}

/// Find a working Python 3 interpreter.
///
/// Tries python3, python, py in order, verifying each outputs "Python 3".
fn find_python_command() -> WaveResult<String> {
    let candidates = ["python3", "python", "py"];
    for candidate in &candidates {
        let result = Command::new(candidate).arg("--version").output();
        if let Ok(output) = result
            && output.status.success()
        {
            let version_str = String::from_utf8_lossy(&output.stdout);
            if version_str.contains("Python 3") {
                return Ok(candidate.to_string());
            }
        }
    }
    Err(WaveAnalyzerError::DepsError {
        message: "Could not find a Python 3 interpreter. Tried: python3, python, py. \
         Install Python 3 and ensure it is on PATH, then run: \
         pip install -r tools/deps-extractor/requirements.txt"
            .to_string(),
    })
}

/// Validate that deps_raw.json has meaningful content.
fn validate_deps_raw_content(path: &PathBuf) -> WaveResult<()> {
    let content = std::fs::read_to_string(path).map_err(|e| WaveAnalyzerError::FileError {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    if content.trim().is_empty() {
        return Err(WaveAnalyzerError::DepsError {
            message: format!(
                "{} is empty (0 bytes). The extraction produced no output.",
                path.display()
            ),
        });
    }

    let json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| WaveAnalyzerError::DepsError {
            message: format!("{} is not valid JSON: {}", path.display(), e),
        })?;

    let edges = json
        .get("edges")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let top_module = json
        .get("top_module")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    if edges == 0 {
        let clocks = json
            .get("clocks")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let ports = json
            .get("boundary_ports")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        return Err(WaveAnalyzerError::DepsError {
            message: format!(
                "deps_raw.json has 0 edges (clocks={}, ports={}, top_module='{}'). \
                 Extraction likely failed to find the top module in the RTL source. \
                 Check that the top module name matches a module definition in the Verilog files.",
                clocks, ports, top_module
            ),
        });
    }

    Ok(())
}

/// Validate that deps.yaml has meaningful content.
fn validate_deps_yaml_content(path: &PathBuf) -> WaveResult<()> {
    let content = std::fs::read_to_string(path).map_err(|e| WaveAnalyzerError::FileError {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;

    if content.trim().is_empty() {
        return Err(WaveAnalyzerError::DepsError {
            message: format!(
                "{} is empty (0 bytes). The converter produced no output.",
                path.display()
            ),
        });
    }

    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&content).map_err(|e| WaveAnalyzerError::DepsError {
            message: format!("{} is not valid YAML: {}", path.display(), e),
        })?;

    let deps = yaml
        .get("dependencies")
        .and_then(|v| v.as_sequence())
        .map(|a| a.len())
        .unwrap_or(0);

    if deps == 0 {
        return Err(WaveAnalyzerError::DepsError {
            message: format!(
                "{} has 0 dependency entries. The conversion produced no dependencies. \
                 This usually means the upstream extraction (deps_raw.json) was empty.",
                path.display()
            ),
        });
    }

    Ok(())
}

/// Run the deps-extractor pipeline
///
/// Steps:
/// 1. Locate deps-extractor scripts
/// 2. Run extraction engine (pyverilog or vivado) to generate deps_raw.json
/// 3. Run deps_converter.py to convert deps_raw.json to deps.yaml
/// 4. Return the path to the generated deps.yaml
pub fn run_deps_extractor(
    rtl_path: &str,
    top_module: &str,
    engine: Option<&str>,
    annotations_path: Option<&str>,
    output_path: Option<&str>,
    deps_extractor_path: Option<&str>,
) -> WaveResult<DepsExtractorResult> {
    let engine = engine.unwrap_or("pyverilog");
    let rtl_path_buf = PathBuf::from(rtl_path);
    let (deps_raw_json, deps_yaml) = resolve_output_paths(&rtl_path_buf, output_path)?;
    let extractor_dir = find_deps_extractor_dir(deps_extractor_path);

    // Step 1: Run extraction engine
    match engine {
        "pyverilog" => {
            let embedded_sidecar = materialize_embedded_sidecar()?;
            let external_sidecar = extractor_dir.as_ref().and_then(|dir| find_sidecar_exe(dir));

            if let Some(sidecar) = embedded_sidecar.or(external_sidecar) {
                run_sidecar_extractor(
                    &sidecar,
                    &rtl_path_buf,
                    top_module,
                    annotations_path,
                    &deps_yaml,
                )?;
                return Ok(DepsExtractorResult {
                    deps_yaml_path: deps_yaml.to_string_lossy().to_string(),
                    engine: engine.to_string(),
                    node_count: None,
                    stdout: format!(
                        "Dependencies extracted successfully.\nEngine: {}\nOutput: {}\nMode: sidecar-exe",
                        engine,
                        deps_yaml.display()
                    ),
                });
            }

            let extractor_dir = extractor_dir.ok_or_else(|| {
                WaveAnalyzerError::DepsError {
                    message: "Could not locate deps-extractor directory for Python fallback. \
                     For single-exe delivery, build tools/deps-extractor/dist/wave-analyzer-deps-extractor.exe \
                     first, then rebuild wave-analyzer-cli.exe so the sidecar is embedded."
                        .to_string(),
                }
            })?;

            run_pyverilog_extraction(&extractor_dir, &rtl_path_buf, top_module, &deps_raw_json)?;

            // Validate deps_raw.json content
            validate_deps_raw_content(&deps_raw_json)?;

            // Run deps_converter.py
            run_deps_converter(&extractor_dir, &deps_raw_json, &deps_yaml, annotations_path)?;

            if !deps_yaml.exists() {
                return Err(WaveAnalyzerError::DepsError {
                    message: format!(
                        "deps_converter.py completed but deps.yaml not found at {}",
                        deps_yaml.display()
                    ),
                });
            }

            validate_deps_yaml_content(&deps_yaml)?;

            return Ok(DepsExtractorResult {
                deps_yaml_path: deps_yaml.to_string_lossy().to_string(),
                engine: engine.to_string(),
                node_count: None,
                stdout: format!(
                    "Dependencies extracted successfully.\nEngine: {}\nOutput: {}",
                    engine,
                    deps_yaml.display()
                ),
            });
        }
        "vivado" => {
            return Err(WaveAnalyzerError::DepsError {
                message: "Vivado extraction requires Vivado installation and .xpr project. Use the wave_analyzer_tool.py Python tool for Vivado-based extraction.".to_string(),
            });
        }
        other => {
            return Err(WaveAnalyzerError::InvalidArgument {
                message: format!(
                    "Unknown extraction engine '{}'. Use 'pyverilog' or 'vivado'.",
                    other
                ),
            });
        }
    }
}

/// Detect iverilog install root from PATH or environment variables.
///
/// Priority:
/// 1. Find `iverilog` on system PATH, return its parent's parent (bin/..)
/// 2. IVERILOG_HOME env var
/// 3. IVERILOG_PATH env var (legacy)
fn detect_iverilog_root() -> Option<PathBuf> {
    // 1. Search system PATH for iverilog executable
    if let Ok(output) = Command::new("where").arg("iverilog").output() {
        if output.status.success() {
            let line = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            if !line.is_empty() {
                let exe_path = PathBuf::from(&line);
                if let Some(root) = exe_path.parent().and_then(|p| p.parent()) {
                    return Some(root.to_path_buf());
                }
            }
        }
    }

    // 2. IVERILOG_HOME
    if let Ok(val) = std::env::var("IVERILOG_HOME") {
        let p = PathBuf::from(&val);
        if p.exists() {
            return Some(p);
        }
    }

    // 3. IVERILOG_PATH (legacy)
    if let Ok(val) = std::env::var("IVERILOG_PATH") {
        let p = PathBuf::from(&val);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

fn run_sidecar_extractor(
    sidecar: &Path,
    rtl_path: &Path,
    top_module: &str,
    annotations_path: Option<&str>,
    output_path: &Path,
) -> WaveResult<()> {
    let mut cmd = Command::new(sidecar);
    cmd.arg("extract-deps")
        .arg("--rtl-path")
        .arg(rtl_path)
        .arg("--top-module")
        .arg(top_module)
        .arg("--output-path")
        .arg(output_path);

    if let Some(ann) = annotations_path {
        cmd.arg("--annotations-path").arg(ann);
    }

    // Pass iverilog install root if detectable from PATH or env vars
    if let Some(iverilog_root) = detect_iverilog_root() {
        cmd.arg("--iverilog-path").arg(&iverilog_root);
    }

    // Force UTF-8 mode: pyverilog uses open() without explicit encoding,
    // which defaults to GBK on Windows and fails on UTF-8 Verilog sources.
    cmd.env("PYTHONUTF8", "1");

    // Forward IVERILOG_HOME / VIVADO_HOME if set (belt-and-suspenders)
    for var in &["IVERILOG_HOME", "IVERILOG_PATH", "VIVADO_HOME"] {
        if let Ok(val) = std::env::var(var) {
            cmd.env(var, &val);
        }
    }

    let result = cmd.output().map_err(|e| WaveAnalyzerError::DepsError {
        message: format!(
            "Failed to run sidecar extractor '{}': {}",
            sidecar.display(),
            e
        ),
    })?;

    if !result.status.success() {
        let code = result.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&result.stdout);
        let stderr = String::from_utf8_lossy(&result.stderr);

        let hint = if code == -1073741515 || code == 0xc0000135_u32 as i32 {
            "Hint: VC++ Runtime is missing. Install from \
             https://aka.ms/vs/17/release/vc_redist.x64.exe"
        } else if stderr.contains("ModuleNotFoundError") || stderr.contains("ImportError") {
            "Hint: sidecar packaging is incomplete. Rebuild with: \
             python tools/deps-extractor/build_sidecar.py && cargo build --release"
        } else if stderr.contains("iverilog") || stderr.contains("IVERILOG") {
            "Hint: iverilog not found. Add to PATH or set \
             IVERILOG_HOME=<install_root>"
        } else if stderr.contains("PermissionError") {
            "Hint: permission denied — check that %TEMP% is writable \
             and antivirus is not blocking the sidecar exe"
        } else {
            "Hint: run `check_env` to diagnose environment configuration"
        };

        return Err(WaveAnalyzerError::DepsError {
            message: format!(
                "sidecar extractor failed (exit code {code}):\n{hint}\n\
                 --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
            ),
        });
    }

    Ok(())
}

fn run_pyverilog_extraction(
    extractor_dir: &Path,
    rtl_path: &Path,
    top_module: &str,
    deps_raw_json: &Path,
) -> WaveResult<()> {
    let pyverilog_script = extractor_dir.join("extract_deps_pyverilog.py");
    if !pyverilog_script.exists() {
        return Err(WaveAnalyzerError::FileError {
            path: pyverilog_script.display().to_string(),
            message: "extract_deps_pyverilog.py not found".to_string(),
        });
    }

    let python_cmd = find_python_command()?;
    let mut cmd = Command::new(&python_cmd);
    cmd.arg(&pyverilog_script)
        .arg(rtl_path)
        .arg("-t")
        .arg(top_module)
        .arg("-o")
        .arg(deps_raw_json)
        .env("PYTHONUTF8", "1");

    let result = cmd.output().map_err(|e| WaveAnalyzerError::DepsError {
        message: format!(
            "Failed to run extract_deps_pyverilog.py with '{}': {}",
            python_cmd, e
        ),
    })?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        // Check if it's an import error and give installation hint
        if stderr.contains("ImportError") || stderr.contains("ModuleNotFoundError") {
            return Err(WaveAnalyzerError::DepsError {
                message: format!(
                    "Python dependency missing. Install with: pip install -r {}/requirements.txt\n\n{}",
                    extractor_dir.display(),
                    stderr
                ),
            });
        }
        return Err(WaveAnalyzerError::DepsError {
            message: format!("extract_deps_pyverilog.py failed:\n{}\n{}", stdout, stderr),
        });
    }

    if !deps_raw_json.exists() {
        return Err(WaveAnalyzerError::DepsError {
            message: "extract_deps_pyverilog.py ran but deps_raw.json was not generated"
                .to_string(),
        });
    }

    Ok(())
}

fn run_deps_converter(
    extractor_dir: &Path,
    deps_raw_json: &Path,
    deps_yaml: &Path,
    annotations_path: Option<&str>,
) -> WaveResult<()> {
    let converter_script = extractor_dir.join("deps_converter.py");
    if !converter_script.exists() {
        return Err(WaveAnalyzerError::FileError {
            path: converter_script.display().to_string(),
            message: "deps_converter.py not found".to_string(),
        });
    }

    let python_cmd = find_python_command()?;
    let mut cmd = Command::new(&python_cmd);
    cmd.arg(&converter_script)
        .arg(deps_raw_json)
        .arg("-o")
        .arg(deps_yaml);

    if let Some(ann) = annotations_path {
        cmd.arg("--annotate").arg(ann);
    }

    let result = cmd.output().map_err(|e| WaveAnalyzerError::DepsError {
        message: format!(
            "Failed to run deps_converter.py with '{}': {}",
            python_cmd, e
        ),
    })?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        if stderr.contains("ImportError") || stderr.contains("ModuleNotFoundError") {
            return Err(WaveAnalyzerError::DepsError {
                message: format!(
                    "Python dependency missing. Install with: pip install -r {}/requirements.txt\n\n{}",
                    extractor_dir.display(),
                    stderr
                ),
            });
        }
        return Err(WaveAnalyzerError::DepsError {
            message: format!("deps_converter.py failed:\n{}\n{}", stdout, stderr),
        });
    }

    Ok(())
}

/// Run a full environment diagnostic check for the CLI `check_env` command.
///
/// Checks: embedded sidecar availability, iverilog, VC++ Runtime, Python fallback.
pub fn check_environment() -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("=== wave-analyzer-cli environment check ===".to_string());
    lines.push(String::new());

    // 1. Embedded sidecar — smoke test the full extraction pipeline
    lines.push("[1/4] Embedded sidecar (pyverilog engine)".to_string());
    match materialize_embedded_sidecar() {
        Ok(Some(path)) => {
            lines.push(format!("  Status : OK — released to {}", path.display()));
            // Run smoke test: check subcommand exercises iverilog → pyverilog → YAML
            let mut cmd = Command::new(&path);
            cmd.arg("check");
            if let Some(iverilog_root) = detect_iverilog_root() {
                cmd.arg("--iverilog-path").arg(&iverilog_root);
            }
            cmd.env("PYTHONUTF8", "1");
            let test = cmd.output();
            match test {
                Ok(out) if out.status.success() => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    for line in stdout.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with('[') {
                            lines.push(format!("  {trimmed}"));
                        }
                    }
                    lines.push("  Engine : HEALTHY".to_string());
                }
                Ok(out) => {
                    let code = out.status.code().unwrap_or(-1);
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if code == -1073741515 || code == 0xc0000135_u32 as i32 {
                        lines.push("  Engine : FAIL — VC++ Runtime missing".to_string());
                        lines.push(
                            "  Fix    : Install https://aka.ms/vs/17/release/vc_redist.x64.exe"
                                .to_string(),
                        );
                    } else {
                        lines.push(format!("  Engine : FAIL — exit code {code}"));
                        // Show smoke test output (which step failed)
                        for line in stdout.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with('[') {
                                lines.push(format!("  {trimmed}"));
                            }
                        }
                        if !stderr.is_empty() {
                            let err_lines: Vec<&str> = stderr.lines().take(5).collect();
                            lines.push(format!("  Stderr : {}", err_lines.join("\n           ")));
                        }
                    }
                }
                Err(e) => {
                    lines.push(format!("  Engine : FAIL — cannot execute: {e}"));
                }
            }
        }
        Ok(None) => {
            lines.push("  Status : NOT EMBEDDED — rebuild with sidecar exe present".to_string());
            lines.push(
                "  Fix    : python tools/deps-extractor/build_sidecar.py && cargo build --release"
                    .to_string(),
            );
        }
        Err(e) => {
            lines.push(format!("  Status : ERROR — {e}"));
        }
    }
    lines.push(String::new());

    // 2. External sidecar (tools/deps-extractor/dist/)
    lines.push("[2/4] External sidecar (tools/deps-extractor/)".to_string());
    let ext_dir = find_deps_extractor_dir(None);
    match ext_dir.as_ref().and_then(|d| find_sidecar_exe(d)) {
        Some(path) => {
            lines.push(format!("  Path   : {}", path.display()));
            lines.push("  Status : OK".to_string());
        }
        None => {
            lines.push("  Status : not found (OK if embedded sidecar works)".to_string());
        }
    }
    lines.push(String::new());

    // 3. iverilog
    lines.push("[3/4] iverilog (Verilog preprocessor)".to_string());
    match detect_iverilog_root() {
        Some(root) => {
            lines.push(format!("  Root   : {}", root.display()));
            // Check actual executable
            let exe = root.join("bin").join("iverilog.exe");
            if exe.exists() {
                lines.push(format!("  Exe    : {} — OK", exe.display()));
            } else {
                let exe_unix = root.join("bin").join("iverilog");
                if exe_unix.exists() {
                    lines.push(format!("  Exe    : {} — OK", exe_unix.display()));
                } else {
                    lines.push("  Exe    : NOT FOUND in bin/".to_string());
                }
            }
        }
        None => {
            lines.push("  Status : NOT FOUND".to_string());
            lines.push(
                "  Fix    : set IVERILOG_HOME=<install_root> or add iverilog to PATH".to_string(),
            );
        }
    }
    lines.push(String::new());

    // 4. VC++ Runtime (Windows only)
    lines.push("[4/4] VC++ Runtime (Windows)".to_string());
    #[cfg(target_os = "windows")]
    {
        let vcruntime = std::path::Path::new("C:\\Windows\\System32\\vcruntime140.dll");
        if vcruntime.exists() {
            lines.push("  Status : OK — vcruntime140.dll found".to_string());
        } else {
            lines.push("  Status : MISSING".to_string());
            lines.push(
                "  Fix    : Install https://aka.ms/vs/17/release/vc_redist.x64.exe".to_string(),
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        lines.push("  Status : N/A (not Windows)".to_string());
    }

    lines.push(String::new());
    lines.push("Done. Fix any FAIL items above, then re-run check_env.".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::resolve_output_paths;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_output_paths_file_target() {
        let dir = TempDir::new().expect("temp dir");
        let rtl = dir.path().join("src").join("led_blink.v");
        std::fs::create_dir_all(rtl.parent().unwrap()).expect("create rtl dir");
        std::fs::write(&rtl, "module led_blink; endmodule").expect("write rtl");

        let (raw, yaml) = resolve_output_paths(
            &rtl,
            Some(dir.path().join("deps_auto.yaml").to_str().unwrap()),
        )
        .expect("resolve");
        assert_eq!(yaml, dir.path().join("deps_auto.yaml"));
        assert_eq!(raw, dir.path().join("deps_raw.json"));
    }

    #[test]
    fn test_resolve_output_paths_existing_directory() {
        let dir = TempDir::new().expect("temp dir");
        let rtl = dir.path().join("led_blink.v");
        std::fs::write(&rtl, "module led_blink; endmodule").expect("write rtl");

        let (raw, yaml) =
            resolve_output_paths(&rtl, Some(dir.path().to_str().unwrap())).expect("resolve");
        assert_eq!(yaml, dir.path().join("deps.yaml"));
        assert_eq!(raw, dir.path().join("deps_raw.json"));
    }

    #[test]
    fn test_resolve_output_paths_extensionless_target() {
        let dir = TempDir::new().expect("temp dir");
        let rtl = dir.path().join("led_blink.v");
        std::fs::write(&rtl, "module led_blink; endmodule").expect("write rtl");

        let (raw, yaml) =
            resolve_output_paths(&rtl, Some(dir.path().join("out").to_str().unwrap()))
                .expect("resolve");
        assert_eq!(yaml, dir.path().join("out").join("deps.yaml"));
        assert_eq!(raw, dir.path().join("out").join("deps_raw.json"));
    }

    #[test]
    fn test_resolve_output_paths_default() {
        let dir = TempDir::new().expect("temp dir");
        let rtl = dir.path().join("rtl").join("led_blink.v");
        std::fs::create_dir_all(rtl.parent().unwrap()).expect("create rtl dir");
        std::fs::write(&rtl, "module led_blink; endmodule").expect("write rtl");

        let (raw, yaml) = resolve_output_paths(&rtl, None).expect("resolve");
        assert_eq!(yaml, dir.path().join("rtl").join("deps.yaml"));
        assert_eq!(raw, dir.path().join("rtl").join("deps_raw.json"));
    }
}
