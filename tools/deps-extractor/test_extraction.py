#!/usr/bin/env python3
"""test_extraction.py — Pyverilog deps-extractor 端到端测试

使用 test_rtl/ 中的 3 个 Verilog 文件验证提取和转换正确性。

运行方式:
    cd wave-analyzer-mcp/tools/deps-extractor
    python -m pytest test_extraction.py -v
"""

import json
import os
import re
import tempfile
from pathlib import Path

import pytest
import yaml

# 导入被测模块
from extract_deps_pyverilog import DataflowAnalyzer
from deps_converter import DepsConverter

TEST_RTL_DIR = Path(__file__).parent / "test_rtl"
LED_BLINK_RTL = Path(r"E:\fpgaProjectTest\led-blink\src\led_blink.v")


# ============================================================
# 辅助函数
# ============================================================

def run_extraction(v_file: str, top_module: str) -> dict:
    """运行 Pyverilog 提取，返回 deps_raw.json 字典。"""
    rtl_path = TEST_RTL_DIR / v_file
    assert rtl_path.exists(), f"RTL file not found: {rtl_path}"

    analyzer = DataflowAnalyzer([str(rtl_path)], top_module)
    return analyzer.analyze()


def run_conversion(raw_data: dict, annotations: dict | None = None) -> dict:
    """运行 deps_converter 转换，返回 deps.yaml 字典。"""
    converter = DepsConverter(raw_data, annotations)
    return converter.convert()


def get_edges_by_target(raw: dict, target_signal: str) -> list[dict]:
    """从 deps_raw 中获取指定目标信号的所有边。"""
    return [e for e in raw["edges"] if e["target"] == target_signal]


def get_deps_by_output(deps: dict, output_signal: str) -> dict | None:
    """从 deps.yaml 中获取指定输出信号的依赖条目。"""
    for entry in deps["dependencies"]:
        if entry["output"] == output_signal:
            return entry
    return None


# ============================================================
# simple_reg 测试
# ============================================================

class TestSimpleReg:
    """简单寄存器：单级 FF + enable 控制。"""

    @pytest.fixture(autouse=True)
    def setup(self):
        self.raw = run_extraction("simple_reg.v", "simple_reg")

    def test_clocks_detected(self):
        assert len(self.raw["clocks"]) == 1
        assert self.raw["clocks"][0]["logical_name"] == "clk"

    def test_boundary_ports(self):
        ports = {p["path"]: p for p in self.raw["boundary_ports"]}
        assert "TOP.data_i" in ports
        assert "TOP.enable" in ports
        assert "TOP.data_o" in ports
        assert ports["TOP.data_i"]["direction"] == "IN"
        assert ports["TOP.data_o"]["direction"] == "OUT"

    def test_sequential_edge(self):
        """data_i → data_o 应为 sequential/FF。"""
        edges = get_edges_by_target(self.raw, "TOP.data_o")
        seq_edges = [e for e in edges if e["inferred_type"] == "sequential"]
        assert len(seq_edges) == 2
        seq_sources = {e["source"] for e in seq_edges}
        assert "TOP.data_i" in seq_sources
        assert "TOP.clk" in seq_sources
        for edge in seq_edges:
            assert edge["inferred_by"] == "FF"
            assert edge["clock"] == "clk"
            assert edge["latency_cycles"] == 1

    def test_control_edge(self):
        """enable → data_o 应为 control/FF_CE。"""
        edges = get_edges_by_target(self.raw, "TOP.data_o")
        ctrl_edges = [e for e in edges if e["inferred_by"] == "FF_CE"]
        assert len(ctrl_edges) >= 1
        assert ctrl_edges[0]["source"] == "TOP.enable"

    def test_conversion_output(self):
        """deps.yaml 应包含正确格式。"""
        deps = run_conversion(self.raw)
        assert deps["format_version"] == "1.0"
        assert len(deps["clock_aliases"]) == 1
        data_o_entry = get_deps_by_output(deps, "TOP.data_o")
        assert data_o_entry is not None
        assert data_o_entry["category"] == "data"
        # 应包含 sequential 和 control 边
        types = [e["type"] for e in data_o_entry["depends_on"]]
        assert "sequential" in types
        assert "control" in types


# ============================================================
# multi_module 测试
# ============================================================

class TestMultiModule:
    """多模块设计：顶层 + 2 个子模块实例。"""

    @pytest.fixture(autouse=True)
    def setup(self):
        self.raw = run_extraction("multi_module.v", "top_multi")

    def test_clock_propagation(self):
        """子模块的 clk 应传播到顶层。"""
        assert len(self.raw["clocks"]) >= 1
        clk_names = [c["logical_name"] for c in self.raw["clocks"]]
        assert "clk" in clk_names

    def test_submodule_sequential_edge(self):
        """子模块内部 u_ch0.data_i → u_ch0.data_o 应为 sequential。"""
        edges = get_edges_by_target(self.raw, "TOP.u_ch0.data_o")
        seq_edges = [e for e in edges if e["inferred_type"] == "sequential"]
        assert len(seq_edges) >= 1, (
            f"Expected sequential edge for u_ch0.data_o, got: "
            f"{[(e['source'], e['inferred_type']) for e in edges]}"
        )
        assert seq_edges[0]["source"] == "TOP.u_ch0.data_i"
        assert seq_edges[0]["clock"] == "clk"

    def test_submodule_control_edge(self):
        """子模块 enable → data_o 应为 control。"""
        edges = get_edges_by_target(self.raw, "TOP.u_ch0.data_o")
        ctrl_edges = [e for e in edges if e["inferred_by"] == "FF_CE"]
        assert len(ctrl_edges) >= 1

    def test_both_channels_present(self):
        """两个通道的边都应存在。"""
        ch0_edges = get_edges_by_target(self.raw, "TOP.u_ch0.data_o")
        ch1_edges = get_edges_by_target(self.raw, "TOP.u_ch1.data_o")
        assert len(ch0_edges) >= 2, "ch0 should have sequential + control edges"
        assert len(ch1_edges) >= 2, "ch1 should have sequential + control edges"

    def test_no_temp_variables(self):
        """不应包含 _rn 临时变量。"""
        for e in self.raw["edges"]:
            assert not re.search(r'_rn\d+_', e["source"]), f"Temp var in source: {e['source']}"
            assert not re.search(r'_rn\d+_', e["target"]), f"Temp var in target: {e['target']}"

    def test_conversion_clock_aliases(self):
        """deps.yaml 应有 clock_aliases。"""
        deps = run_conversion(self.raw)
        assert len(deps["clock_aliases"]) >= 1
        clk_names = [c["clock_name"] for c in deps["clock_aliases"]]
        assert "clk" in clk_names

    def test_no_wire_passthrough_edges(self):
        """不应有 wire 穿越边（如 clk→u_ch0.clk）。"""
        for e in self.raw["edges"]:
            src_base = e["source"].split(".")[-1]
            tgt_base = e["target"].split(".")[-1]
            if e["inferred_by"] == "NET" and e["inferred_type"] == "combinational":
                assert src_base != tgt_base, (
                    f"Wire passthrough edge found: {e['source']} -> {e['target']}"
                )

    def test_ce_edge_dedup(self):
        """每个子模块只应有 1 条 CE 边。"""
        ch0_edges = get_edges_by_target(self.raw, "TOP.u_ch0.data_o")
        ce_edges = [e for e in ch0_edges if e["inferred_by"] == "FF_CE"]
        assert len(ce_edges) == 1, f"Expected 1 CE edge for ch0, got {len(ce_edges)}"

    def test_ce_path_uses_instance_prefix(self):
        """CE 边 source 应使用实例路径（如 TOP.u_ch0.enable）。"""
        ch0_edges = get_edges_by_target(self.raw, "TOP.u_ch0.data_o")
        ce_edges = [e for e in ch0_edges if e["inferred_by"] == "FF_CE"]
        assert len(ce_edges) >= 1
        assert "u_ch0" in ce_edges[0]["source"], (
            f"CE source should use instance prefix, got: {ce_edges[0]['source']}"
        )

    def test_clean_edge_count(self):
        """multi_module 应包含每通道 clk + seq + ctrl 3 条边。"""
        assert len(self.raw["edges"]) == 6, (
            f"Expected 6 clean edges, got {len(self.raw['edges'])}: "
            f"{[(e['source'], e['target']) for e in self.raw['edges']]}"
        )


# ============================================================
# fsm_counter 测试
# ============================================================

class TestFsmCounter:
    """FSM + case 语句 + 组合逻辑。"""

    @pytest.fixture(autouse=True)
    def setup(self):
        self.raw = run_extraction("fsm_counter.v", "fsm_counter")

    def test_clock_detected(self):
        assert len(self.raw["clocks"]) == 1
        assert self.raw["clocks"][0]["logical_name"] == "clk"

    def test_no_temp_variables(self):
        """不应包含 _rn 临时变量。"""
        for e in self.raw["edges"]:
            assert not re.search(r'_rn\d+_', e["source"]), f"Temp var in source: {e['source']}"
            assert not re.search(r'_rn\d+_', e["target"]), f"Temp var in target: {e['target']}"

    def test_no_constants_as_signals(self):
        """localparam 常量不应作为 source 信号。"""
        constant_names = {"S_IDLE", "S_RUN", "S_DONE"}
        for e in self.raw["edges"]:
            src_base = e["source"].split(".")[-1]
            assert src_base not in constant_names, (
                f"Constant '{src_base}' found as source in edge: {e['source']} -> {e['target']}"
            )

    def test_state_sequential_edge(self):
        """next_state → state 应为 sequential。"""
        edges = get_edges_by_target(self.raw, "TOP.state")
        seq_edges = [e for e in edges if e["inferred_type"] == "sequential"]
        assert len(seq_edges) >= 1
        sources = [e["source"] for e in seq_edges]
        assert "TOP.next_state" in sources

    def test_count_self_feedback(self):
        """count 应有自回馈 sequential 边。"""
        edges = get_edges_by_target(self.raw, "TOP.count")
        self_fb = [e for e in edges
                   if e["source"] == "TOP.count" and e["inferred_type"] == "sequential"]
        assert len(self_fb) >= 1, (
            f"Expected count self-feedback edge, got: "
            f"{[(e['source'], e['inferred_type']) for e in edges]}"
        )

    def test_next_state_combinational(self):
        """next_state 应有组合依赖（来自 state 和 start）。"""
        edges = get_edges_by_target(self.raw, "TOP.next_state")
        sources = [e["source"] for e in edges]
        assert "TOP.start" in sources or "TOP.state" in sources, (
            f"next_state should depend on start or state, got: {sources}"
        )

    def test_threshold_guard_dependency(self):
        """count 寄存器应有 threshold 作为 guard 组合依赖。"""
        edges = get_edges_by_target(self.raw, "TOP.count")
        sources = [e["source"] for e in edges]
        assert "TOP.threshold" in sources, (
            f"count should have threshold as guard dependency, got: {sources}"
        )

    def test_done_signal_present(self):
        """done 信号应有组合依赖（来自 state）。"""
        edges = get_edges_by_target(self.raw, "TOP.done")
        assert len(edges) >= 1, "done signal should have at least one edge"
        sources = [e["source"] for e in edges]
        assert "TOP.state" in sources, (
            f"done should depend on state, got: {sources}"
        )

    def test_conversion_output(self):
        """deps.yaml 转换应成功且无 _rn 残留。"""
        deps = run_conversion(self.raw)
        assert deps["format_version"] == "1.0"
        # 检查所有依赖条目无 _rn 残留
        for entry in deps["dependencies"]:
            assert not re.search(r'_rn\d+_', entry["output"]), (
                f"Temp var in output: {entry['output']}"
            )
            for edge in entry["depends_on"]:
                assert not re.search(r'_rn\d+_', edge["signal"]), (
                    f"Temp var in signal: {edge['signal']}"
                )


# ============================================================
# deps_converter 独立测试
# ============================================================

class TestDepsConverter:
    """deps_converter.py 单元级测试。"""

    def test_empty_input_raises(self):
        """空 deps_raw 应抛出 ValueError。"""
        raw = {"edges": [], "clocks": [], "boundary_ports": [], "top_module": "empty"}
        converter = DepsConverter(raw)
        with pytest.raises(ValueError, match="empty"):
            converter.convert()

    def test_temp_variable_filtering(self):
        """converter 应过滤 _rn 临时变量。"""
        raw = {
            "extractor": "pyverilog",
            "top_module": "test",
            "clocks": [],
            "boundary_ports": [
                {"path": "TOP.a", "direction": "IN", "width": 1, "kind": "input_port"},
                {"path": "TOP.b", "direction": "OUT", "width": 1, "kind": "output_port"},
            ],
            "edges": [
                {"source": "TOP.a", "target": "TOP.b", "inferred_type": "combinational",
                 "inferred_by": "NET", "clock": None, "clock_edge": None,
                 "latency_cycles": 0, "details": ""},
                # 临时变量边，应被过滤
                {"source": "TOP._rn0_temp", "target": "TOP.b", "inferred_type": "combinational",
                 "inferred_by": "DATAFLOW", "clock": None, "clock_edge": None,
                 "latency_cycles": 0, "details": ""},
            ],
        }
        converter = DepsConverter(raw)
        deps = converter.convert()
        b_entry = get_deps_by_output(deps, "TOP.b")
        assert b_entry is not None
        # 只应有 1 条边（_rn 被过滤）
        assert len(b_entry["depends_on"]) == 1
        assert b_entry["depends_on"][0]["signal"] == "TOP.a"

    def test_annotations_constants(self):
        """annotations 中的 constants 应生成 boundary 节点。"""
        raw = {
            "extractor": "pyverilog",
            "top_module": "test",
            "clocks": [],
            "boundary_ports": [
                {"path": "TOP.a", "direction": "IN", "width": 1, "kind": "input_port"},
            ],
            "edges": [
                {"source": "TOP.a", "target": "TOP.a", "inferred_type": "boundary",
                 "inferred_by": "NET", "clock": None, "clock_edge": None,
                 "latency_cycles": 0, "details": ""},
            ],
        }
        annotations = {
            "constants": [
                {"signal": "TOP.CFG_MODE", "value": "2'b01", "description": "Fixed mode"},
            ]
        }
        converter = DepsConverter(raw, annotations)
        deps = converter.convert()
        cfg_entry = get_deps_by_output(deps, "TOP.CFG_MODE")
        assert cfg_entry is not None
        assert cfg_entry["depends_on"][0]["type"] == "boundary"
        assert cfg_entry["depends_on"][0]["boundary_kind"] == "constant"


class TestLedBlinkRegression:
    """Regression coverage for led_blink auto deps extraction."""

    @pytest.fixture(autouse=True)
    def setup(self, monkeypatch):
        if not LED_BLINK_RTL.exists():
            pytest.skip(f"RTL file not found: {LED_BLINK_RTL}")
        tmpdir = Path(__file__).resolve().parents[2] / "target" / "tmp"
        tmpdir.mkdir(parents=True, exist_ok=True)
        monkeypatch.setenv("TEMP", str(tmpdir))
        monkeypatch.setenv("TMP", str(tmpdir))
        self.raw = DataflowAnalyzer([str(LED_BLINK_RTL)], "led_blink").analyze()
        self.deps = DepsConverter(self.raw).convert()

    def test_extracts_ff_and_reset_edges(self):
        counter_edges = get_edges_by_target(self.raw, "TOP.counter")
        led_edges = get_edges_by_target(self.raw, "TOP.led")

        assert any(e["inferred_by"] == "FF" and e["source"] == "TOP.counter" for e in counter_edges)
        assert any(e["inferred_by"] == "FF" and e["source"] == "TOP.clk" for e in counter_edges)
        assert any(e["inferred_by"] == "FF" and e["source"] == "TOP.led" for e in led_edges)
        assert any(e["inferred_type"] == "combinational" and e["source"] == "TOP.counter" for e in led_edges)
        assert any(e["inferred_by"] == "FF" and e["source"] == "TOP.clk" for e in led_edges)
        assert any(e["inferred_by"] == "FF_RST" and e["clock"] == "clk" for e in counter_edges)
        assert any(e["inferred_by"] == "FF_RST" and e["clock"] == "clk" for e in led_edges)

    def test_converts_modelsim_aliases_for_waveform_paths(self):
        aliases = {entry["canonical"]: entry["modelsim"] for entry in self.deps["signal_aliases"]}
        clocks = {entry["clock_name"]: entry["modelsim"] for entry in self.deps["clock_aliases"]}

        assert aliases["TOP.led"] == "led_blink_tb.dut.led"
        assert aliases["TOP.counter"] == "led_blink_tb.dut.counter"
        assert clocks["clk"] == "led_blink_tb.dut.clk"

    def test_rst_control_edges_keep_clock_metadata(self):
        counter_entry = get_deps_by_output(self.deps, "TOP.counter")
        led_entry = get_deps_by_output(self.deps, "TOP.led")

        counter_rst = [e for e in counter_entry["depends_on"] if e["signal"] == "TOP.rst" and e["type"] == "control"]
        led_rst = [e for e in led_entry["depends_on"] if e["signal"] == "TOP.rst" and e["type"] == "control"]

        assert counter_rst and counter_rst[0]["clock"] == "clk"
        assert led_rst and led_rst[0]["clock"] == "clk"


class TestShiftExpressionRegression:
    """Regression coverage for Pyverilog shift-expression handling."""

    def test_shift_expression_does_not_crash_and_keeps_sources(self):
        rtl = """
module shift_top (
    input  wire [7:0] data_i,
    input  wire [2:0] shift_amt,
    output wire [7:0] data_o
    );
assign data_o = data_i << shift_amt;
endmodule
"""
        tmpdir = Path(__file__).resolve().parents[2] / "target" / "tmp"
        tmpdir.mkdir(parents=True, exist_ok=True)
        os.environ["TEMP"] = str(tmpdir)
        os.environ["TMP"] = str(tmpdir)
        os.environ["TMPDIR"] = str(tmpdir)
        with tempfile.NamedTemporaryFile("w", suffix=".v", delete=False, encoding="utf-8") as f:
            f.write(rtl)
            rtl_path = Path(f.name)

        try:
            raw = DataflowAnalyzer([str(rtl_path)], "shift_top").analyze()
        finally:
            rtl_path.unlink(missing_ok=True)

        edges = get_edges_by_target(raw, "TOP.data_o")
        sources = {e["source"] for e in edges}
        assert "TOP.data_i" in sources
        assert "TOP.shift_amt" in sources
