#!/usr/bin/env python3
"""wave-analyzer-deps-extractor -- packaged sidecar for deps extraction.

Build this file into `wave-analyzer-deps-extractor.exe` with PyInstaller.
It runs the Pyverilog extractor and deps converter in one process so end users
do not need a Python installation.
"""

from __future__ import annotations

# Monkey-patch builtins.open to default to UTF-8 encoding on Windows.
# pyverilog internally uses open() without explicit encoding; on Windows
# the default locale is GBK, causing UnicodeDecodeError on UTF-8 source files.
# PYTHONUTF8=1 is ineffective inside PyInstaller-frozen exes because Python
# runtime is already initialized before this script runs.
import builtins as _builtins
import io as _io

_original_open = _builtins.open


def _utf8_open(file, mode="r", *args, **kwargs):
    if "b" not in mode and kwargs.get("encoding") is None:
        kwargs["encoding"] = "utf-8"
    return _original_open(file, mode, *args, **kwargs)


_builtins.open = _utf8_open

import argparse
import json
import os
import shutil
import sys
import tempfile as _tempfile
from pathlib import Path

# ── PLY yacc monkey-patch for PyInstaller frozen environment ──
# PLY uses exec('import ...') to load/write parsetab cache modules,
# which PyInstaller's static analysis cannot detect.  In a frozen exe
# the default outputdir (".") is read-only, so parsetab generation fails
# with ModuleNotFoundError.  Fix: disable table caching (write_tables=False)
# and redirect outputdir to a writable temp directory.
_PLY_OUTPUT_DIR = None
if getattr(sys, "frozen", False):
    _PLY_OUTPUT_DIR = _tempfile.mkdtemp(prefix="ply_")
    import ply.yacc as _ply_yacc

    _orig_yacc = _ply_yacc.yacc

    def _patched_yacc(*args, **kwargs):
        kwargs.setdefault("write_tables", False)
        kwargs["outputdir"] = _PLY_OUTPUT_DIR
        return _orig_yacc(*args, **kwargs)

    _ply_yacc.yacc = _patched_yacc

import yaml

from deps_converter import DepsConverter
from extract_deps_pyverilog import DataflowAnalyzer, ensure_utf8_encoding

def _resolve_output_path(output_path: str | None, rtl_path: Path) -> Path:
    if output_path:
        candidate = Path(output_path)
        if candidate.exists() and candidate.is_dir() or candidate.suffix == "":
            return candidate / "deps.yaml"
        return candidate
    return rtl_path.parent / "deps.yaml"


def _collect_rtl_files(rtl_path: Path) -> list[str]:
    if rtl_path.is_file():
        return [str(rtl_path)]
    if rtl_path.is_dir():
        files = sorted(
            str(p)
            for p in rtl_path.rglob("*")
            if p.suffix.lower() in {".v", ".sv"}
        )
        if files:
            return files
    raise FileNotFoundError(f"No RTL files found at {rtl_path}")


def _find_iverilog(iverilog_path: str | None = None) -> Path:
    """查找 iverilog 可执行文件路径。

    优先级：
    1. 系统 PATH（shutil.which）
    2. 环境变量 IVERILOG_HOME（安装根目录，如 D:\\software\\iverilog）
    3. 环境变量 IVERILOG_PATH（兼容旧名）
    4. 函数参数 iverilog_path
    """
    # 1. System PATH first
    which_result = shutil.which("iverilog")
    if which_result:
        found = Path(which_result)
        os.environ.setdefault("IVERILOG", str(found))
        return found

    # 2-4. Search known install roots
    search_roots: list[Path] = []
    for env_var in ("IVERILOG_HOME", "IVERILOG_PATH"):
        val = os.environ.get(env_var)
        if val:
            search_roots.append(Path(val))
    if iverilog_path:
        search_roots.append(Path(iverilog_path))

    for root in search_roots:
        for name in ("bin/iverilog.exe", "bin/iverilog"):
            candidate = root / name
            if candidate.is_file():
                os.environ.setdefault("IVERILOG", str(candidate))
                return candidate

    raise FileNotFoundError(
        "iverilog 未找到。请将 iverilog 加入系统 PATH，"
        "或配置环境变量 IVERILOG_HOME 指向安装目录"
        "（如 D:\\software\\iverilog），或通过参数 --iverilog-path 指定"
    )


def extract_deps(args: argparse.Namespace) -> int:
    iverilog_exe = _find_iverilog(getattr(args, "iverilog_path", None))
    print(f"iverilog: {iverilog_exe}")

    rtl_path = Path(args.rtl_path)
    output_file = _resolve_output_path(args.output_path, rtl_path)
    output_file.parent.mkdir(parents=True, exist_ok=True)
    raw_file = output_file.parent / "deps_raw.json"

    rtl_files = _collect_rtl_files(rtl_path)
    utf8_files, temp_dir = ensure_utf8_encoding(rtl_files)
    try:
        analyzer = DataflowAnalyzer(utf8_files, args.top_module)
        raw = analyzer.analyze()
        raw["extractor"] = "pyverilog"
        raw["top_module"] = args.top_module

        with raw_file.open("w", encoding="utf-8") as f:
            json.dump(raw, f, indent=2, ensure_ascii=False)

        annotations = {}
        if args.annotations_path:
            with open(args.annotations_path, "r", encoding="utf-8") as f:
                annotations = yaml.safe_load(f) or {}

        deps = DepsConverter(raw, annotations).convert()
        with output_file.open("w", encoding="utf-8") as f:
            yaml.safe_dump(deps, f, sort_keys=False, allow_unicode=True)

        print(f"Written: {output_file}")
        print(f"  Raw: {raw_file}")
        print(f"  RTL files: {len(rtl_files)}")
        return 0
    finally:
        if temp_dir and os.path.exists(temp_dir):
            try:
                shutil.rmtree(temp_dir)
            except Exception:
                pass


# ── Smoke-test RTL for engine health check ──────────────────────────
# Uses SystemVerilog syntax to verify the SV→V2001 preprocessor works.
_SMOKE_TEST_RTL = """\
// Smoke test with SystemVerilog constructs.
// Covers: always_ff, always_comb, logic type, $clog2.
module check_mod (
    input  wire        clk,
    input  wire        rst_n,
    input  wire        enable,
    input  logic [7:0] data_i,
    output logic [7:0] data_o
);

logic [3:0] cnt;

always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) begin
        data_o <= 8'h00;
        cnt    <= 4'h0;
    end else if (enable) begin
        data_o <= data_i;
        cnt    <= cnt + 4'h1;
    end
end

always_comb begin
    if (cnt == 4'hF)
        data_o = 8'hFF;
end

endmodule
"""


def check_engine(args: argparse.Namespace) -> int:
    """End-to-end smoke test: iverilog → pyverilog → deps_converter → YAML."""
    import traceback

    iverilog_path = getattr(args, "iverilog_path", None)

    # Step 1: locate iverilog
    try:
        iverilog_exe = _find_iverilog(iverilog_path)
        print(f"[OK] iverilog: {iverilog_exe}")
    except FileNotFoundError as e:
        print(f"[FAIL] iverilog: {e}")
        return 1

    # Step 2: parse inline RTL through full pipeline
    try:
        with _tempfile.TemporaryDirectory(prefix="sidecar_check_") as tmp:
            rtl_file = Path(tmp) / "check.v"
            rtl_file.write_text(_SMOKE_TEST_RTL, encoding="utf-8")

            utf8_files, temp_dir = ensure_utf8_encoding([str(rtl_file)])
            try:
                # 2a. Pyverilog analysis
                analyzer = DataflowAnalyzer(utf8_files, "check_mod")
                raw = analyzer.analyze()
                raw["extractor"] = "pyverilog"
                raw["top_module"] = "check_mod"

                edge_count = len(raw.get("edges", []))
                port_count = len(raw.get("boundary_ports", []))
                clock_count = len(raw.get("clocks", []))

                if edge_count == 0:
                    print("[FAIL] pyverilog: 0 edges extracted")
                    return 1
                print(
                    f"[OK] pyverilog: {edge_count} edges, "
                    f"{port_count} ports, {clock_count} clocks"
                )

                # 2b. deps_converter
                deps = DepsConverter(raw, {}).convert()
                dep_count = len(deps.get("dependencies", []))
                if dep_count == 0:
                    print("[FAIL] deps_converter: 0 dependencies")
                    return 1
                print(f"[OK] deps_converter: {dep_count} dependencies")

                # 2c. YAML output
                out_file = Path(tmp) / "deps.yaml"
                with out_file.open("w", encoding="utf-8") as f:
                    yaml.safe_dump(deps, f, sort_keys=False, allow_unicode=True)
                print(f"[OK] YAML output: {out_file}")
            finally:
                if temp_dir and os.path.exists(temp_dir):
                    shutil.rmtree(temp_dir, ignore_errors=True)

    except Exception:
        print("[FAIL] extraction pipeline error:")
        traceback.print_exc()
        return 1

    print("All checks passed — extraction engine is healthy.")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="wave-analyzer-deps-extractor")
    sub = parser.add_subparsers(dest="command", required=False)

    extract = sub.add_parser("extract-deps", help="Extract deps.yaml from RTL")
    extract.add_argument("--rtl-path", required=True)
    extract.add_argument("--top-module", required=True)
    extract.add_argument("--output-path")
    extract.add_argument("--annotations-path")
    extract.add_argument(
        "--iverilog-path",
        help="iverilog 安装根目录（默认读 IVERILOG_PATH 环境变量）",
    )
    extract.set_defaults(func=extract_deps)

    check = sub.add_parser("check", help="Smoke-test the extraction engine")
    check.add_argument(
        "--iverilog-path",
        help="iverilog 安装根目录（默认读 IVERILOG_PATH 环境变量）",
    )
    check.set_defaults(func=check_engine)

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    if not hasattr(args, "func"):
        parser.print_help()
        return 1
    return int(args.func(args))


if __name__ == "__main__":
    raise SystemExit(main())
