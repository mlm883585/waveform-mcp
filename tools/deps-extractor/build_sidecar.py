"""
Build wave-analyzer-deps-extractor.exe with PyInstaller.

Usage:
  python build_sidecar.py
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent
ENTRY = ROOT / "sidecar_main.py"
DIST = ROOT / "dist"


def main() -> int:
    cmd = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--onefile",
        "--console",
        "--name",
        "wave-analyzer-deps-extractor",
        "--distpath",
        str(DIST),
        "--workpath",
        str(ROOT / "build"),
        "--specpath",
        str(ROOT / "build"),
        "--clean",
        "--noconfirm",
        "--hidden-import",
        "yaml",
        # PLY (Python Lex-Yacc) — dynamically imported by pyverilog,
        # invisible to PyInstaller's static analysis.
        "--hidden-import",
        "ply",
        "--hidden-import",
        "ply.lex",
        "--hidden-import",
        "ply.yacc",
        # pyverilog submodules — some are imported lazily or via
        # exec('import ...') by PLY's parsetab mechanism.
        "--hidden-import",
        "pyverilog",
        "--hidden-import",
        "pyverilog.vparser",
        "--hidden-import",
        "pyverilog.vparser.parser",
        "--hidden-import",
        "pyverilog.vparser.lexer",
        "--hidden-import",
        "pyverilog.vparser.preprocessor",
        "--hidden-import",
        "pyverilog.vparser.ast",
        "--hidden-import",
        "pyverilog.dataflow",
        "--hidden-import",
        "pyverilog.dataflow.dataflow_analyzer",
        "--hidden-import",
        "pyverilog.dataflow.modulevisitor",
        "--hidden-import",
        "pyverilog.dataflow.signalvisitor",
        "--hidden-import",
        "pyverilog.dataflow.bindvisitor",
        "--hidden-import",
        "pyverilog.dataflow.visit",
        "--hidden-import",
        "pyverilog.dataflow.dataflow",
        # Collect all pyverilog package files (modules + data like VERSION,
        # template/*.txt).  --collect-data alone misses .py submodules that
        # PLY tries to exec-import at runtime.
        "--collect-all",
        "pyverilog",
        "--add-data",
        f"{ROOT / 'extract_deps_pyverilog.py'};.",
        "--add-data",
        f"{ROOT / 'deps_converter.py'};.",
        "--add-data",
        f"{ROOT / 'requirements.txt'};.",
        ENTRY.name,
    ]
    subprocess.run(cmd, cwd=str(ROOT), check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
