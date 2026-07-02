#!/usr/bin/env python3
"""deps_converter.py -- 将 Vivado Tcl 或 Pyverilog 提取的 deps_raw.json
转换为符合 DEPS_FORMAT.md 规范的 deps.yaml。

用法:
    python deps_converter.py deps_raw.json -o deps.yaml [--annotate annotations.yaml] [--format json|yaml]
    python deps_converter.py deps_raw.json --diff deps.yaml  # 增量变更报告
"""

import argparse
import json
import re
import sys
from collections import defaultdict
from datetime import datetime, timezone

# Pyverilog dataflow 内部临时变量前缀（case 语句展开产物）
_PYVERILOG_TEMP_RE = re.compile(r'_rn\d+_')

try:
    import yaml
except ImportError:
    print("Error: PyYAML is required. Install with: pip install pyyaml", file=sys.stderr)
    sys.exit(1)


# ============================================================
# 依赖类型分类映射
# ============================================================

TYPE_MAP = {
    "FF": "sequential",
    "FF_CE": "control",
    "FF_RST": "control",
    "BRAM": "memory",
    "BRAM_EN": "control",
    "NET": "combinational",
    "DATAFLOW": "combinational",
}

def canonical_clock_name(clock: str | None) -> str | None:
    if not clock:
        return None
    name = clock.split("/")[-1].split(".")[-1]
    return name if not re.search(r'(?i)(\brst(?:_|$)|\breset(?:_|$)|\barst(?:_|$))', name) else None


def derive_check_from_condition(expr: str) -> str | None:
    """从简单条件表达式推导 check 值。

    仅处理单信号条件：
    - TOP.enable → ">0"
    - !(TOP.enable) / !TOP.enable → "==0"
    - ~(TOP.signal) / ~TOP.signal → "==0"
    - 复合表达式 → None（仅依赖 condition_expression）
    """
    if not expr:
        return None

    # Strip outer parentheses
    s = expr.strip()
    while s.startswith("(") and s.endswith(")"):
        inner = s[1:-1].strip()
        # Only strip if the parens are balanced and meaningful
        if inner.count("(") == inner.count(")"):
            s = inner
        else:
            break

    # Pattern: !(TOP.xxx) or !TOP.xxx
    negated_match = re.match(r'^!\s*(?:\(?\s*(TOP\.[a-zA-Z_][a-zA-Z0-9_.]*(?:\[\d+(?:\:\d+)?\])?)\s*\)?)\s*$', s)
    if negated_match:
        return "==0"

    # Pattern: ~(TOP.xxx) or ~TOP.xxx
    bitwise_negated_match = re.match(r'^~\s*(?:\(?\s*(TOP\.[a-zA-Z_][a-zA-Z0-9_.]*(?:\[\d+(?:\:\d+)?\])?)\s*\)?)\s*$', s)
    if bitwise_negated_match:
        return "==0"

    # Pattern: bare signal TOP.xxx (possibly with bit select)
    bare_match = re.match(r'^TOP\.[a-zA-Z_][a-zA-Z0-9_.]*(?:\[\d+(?:\:\d+)?\])?\s*$', s)
    if bare_match:
        return ">0"

    # Compound expression — no simple check applicable
    return None


DEFAULT_LATENCY = {
    "sequential": 1,
    "memory": 2,
    "control": 0,
    "combinational": 0,
    "boundary": 0,
    "protocol": 0,
}

# ============================================================
# Category 推断规则
# ============================================================

CATEGORY_PATTERNS = [
    (r"(?i)(valid|ready|enable|en$|sel|state|fsm)", "control"),
    (r"(?i)(\bst\b)", "state"),
    (r"(?i)(data|din|dout|addr|beam)", "data"),
    (r"(?i)(clk|clock)", "control"),
    (r"(?i)(rst|reset)", "control"),
    (r"(?i)(pipe|stage|reg)", "data"),
]


def infer_category(signal_path: str, inferred_type: str) -> str:
    """基于信号名和依赖类型推断 category。

    推断优先级：
    1. 信号名模式匹配（state > control > data）
    2. 依赖类型推断（memory > protocol）
    3. 默认 data
    """
    # 先检查 state（优先级高于 control）
    if re.search(r"(?i)(\bst\b|_st$|state|fsm)", signal_path):
        return "state"
    # 再检查其他模式
    for pattern, cat in CATEGORY_PATTERNS:
        if re.search(pattern, signal_path):
            return cat
    if inferred_type == "memory":
        return "memory"
    if inferred_type == "protocol":
        return "protocol"
    return "data"


# ============================================================
# Canonical 命名生成
# ============================================================

def vivado_to_canonical(path: str) -> str:
    """将 Vivado elaborate 路径转换为 canonical 名。"""
    # generate 索引 → 逻辑通道号: gen_ch__0 → ch0
    path = re.sub(r"\.gen_ch__(\d+)", r".ch\1", path)
    path = re.sub(r"^gen_ch__(\d+)", r"ch\1", path)
    # 实例前缀 u_ → 逻辑名
    path = re.sub(r"\.u_([a-zA-Z])", r".\1", path)
    path = re.sub(r"^u_([a-zA-Z])", r"\1", path)
    return path


def canonical_to_modelsim(canonical: str, top_module: str | None = None) -> str:
    """Map canonical TOP.* paths to the expected ModelSim DUT instance path."""
    if canonical == "TOP":
        return f"{top_module}_tb.dut" if top_module else canonical
    if canonical.startswith("TOP.") and top_module:
        return f"{top_module}_tb.dut." + canonical[4:]
    return canonical


# ============================================================
# 主转换流水线
# ============================================================

class DepsConverter:
    def __init__(self, raw_data: dict, annotations: dict | None = None):
        self.raw = raw_data
        self.annotations = annotations or {}
        self.output_edges: dict[str, list[dict]] = defaultdict(list)
        self.clock_aliases: list[dict] = []
        self.signal_aliases: list[dict] = {}  # canonical -> modelsim path
        self.categories: dict[str, str] = {}
        self._is_vivado = self.raw.get("extractor", "") == "vivado_tcl"
        self._top_module = self.raw.get("top_module", "")

    def _canonicalize(self, path: str) -> str:
        """Convert a raw path to canonical form.
        For Vivado-extracted data, apply vivado_to_canonical regex stripping.
        For Pyverilog-extracted data, paths are already in canonical form.
        """
        if self._is_vivado:
            return vivado_to_canonical(path)
        return path

    def convert(self) -> dict:
        """执行完整转换流水线，返回 deps.yaml 字典。"""
        # 验证输入有实质内容
        edge_count = len(self.raw.get("edges", []))
        clock_count = len(self.raw.get("clocks", []))
        port_count = len(self.raw.get("boundary_ports", []))

        if edge_count == 0 and clock_count == 0 and port_count == 0:
            raise ValueError(
                f"deps_raw.json is empty: 0 edges, 0 clocks, 0 boundary_ports. "
                f"Top module: '{self.raw.get('top_module', 'unknown')}'. "
                f"Extraction likely failed to find the top module in the RTL source."
            )

        if edge_count == 0:
            print(f"Warning: 0 edges in deps_raw.json (clocks={clock_count}, ports={port_count}). "
                  f"Output will have no dependency entries.", file=sys.stderr)

        # 1. 解析 clocks → clock_aliases
        self._build_clock_aliases()

        # 2. 解析 boundary_ports → boundary 节点
        self._build_boundary_nodes()

        # 3. 遍历 edges → deps.yaml 依赖类型
        self._convert_edges()

        # 4. 合并 annotations
        self._apply_annotations()

        # 5. 生成 signal_aliases
        self._build_signal_aliases()

        # 6. 推断 category
        self._infer_categories()

        # 7. 组装输出
        return self._assemble_output()

    # --------------------------------------------------------
    # 1. clock_aliases
    # --------------------------------------------------------
    def _build_clock_aliases(self):
        for clk in self.raw.get("clocks", []):
            logical = clk.get("logical_name", "")
            waveform = clk.get("waveform_path", "")
            if logical and canonical_clock_name(logical):
                canonical_waveform = self._canonicalize(waveform) if waveform else logical
                self.clock_aliases.append({
                    "clock_name": logical,
                    "modelsim": canonical_to_modelsim(canonical_waveform, self._top_module),
                })

    # --------------------------------------------------------
    # 2. boundary_ports → boundary 节点
    # --------------------------------------------------------
    def _build_boundary_nodes(self):
        for bp in self.raw.get("boundary_ports", []):
            path = bp.get("path", "")
            kind = bp.get("kind", "")
            if kind == "input_port":
                canonical = self._canonicalize(path)
                edge = {
                    "signal": canonical,
                    "type": "boundary",
                    "boundary_kind": "input_port",
                    "clock": None,
                    "edge": None,
                    "latency_cycles": 0,
                    "check": None,
                }
                self.output_edges.setdefault(canonical, []).append(edge)
                # input_port 的 category 从信号名推断，但通常是 control 或 data
                self.categories[canonical] = infer_category(canonical, "boundary")
                self.signal_aliases[canonical] = canonical_to_modelsim(canonical, self._top_module)
            elif kind == "output_port":
                # OUT 端口也记录为潜在输出节点（无上游依赖，后续由 edges 填充）
                canonical = self._canonicalize(path)
                self.signal_aliases[canonical] = canonical_to_modelsim(canonical, self._top_module)

    # --------------------------------------------------------
    # 3. edges 转换
    # --------------------------------------------------------
    def _convert_edges(self):
        # 构建 boundary_ports 查找表（用于快速判断）
        boundary_paths = set()
        for bp in self.raw.get("boundary_ports", []):
            boundary_paths.add(bp.get("path", ""))

        for e in self.raw.get("edges", []):
            source = e.get("source", "")
            target = e.get("target", "")
            inferred_type = e.get("inferred_type", "combinational")
            inferred_by = e.get("inferred_by", "NET")
            clock = e.get("clock")
            clock_edge = e.get("clock_edge")
            latency = e.get("latency_cycles", 0)
            details = e.get("details", "")

            if not source or not target:
                continue

            # 过滤 Pyverilog 内部临时变量（兜底保护）
            if _PYVERILOG_TEMP_RE.search(source) or _PYVERILOG_TEMP_RE.search(target):
                continue

            # 类型映射
            dep_type = TYPE_MAP.get(inferred_by, inferred_type)

            # 默认 latency
            if latency == 0 and dep_type in ("sequential", "memory"):
                latency = DEFAULT_LATENCY.get(dep_type, 0)

            canonical_source = self._canonicalize(source)
            canonical_target = self._canonicalize(target)

            edge = {
                "signal": canonical_source,
                "type": dep_type,
            }

            # 时钟逻辑名提取：去掉 Vivado 路径前缀
            clock_logical = canonical_clock_name(clock)

            # 时序相关字段设置
            if dep_type in ("sequential", "memory"):
                edge["clock"] = clock_logical
                edge["edge"] = clock_edge if clock_edge else "posedge"
                edge["latency_cycles"] = latency
            elif dep_type == "control":
                edge["clock"] = clock_logical
                edge["edge"] = clock_edge if clock_edge else None
                edge["latency_cycles"] = latency
            elif dep_type == "boundary":
                edge["boundary_kind"] = "input_port" if source in boundary_paths else None
                edge["clock"] = None
                edge["edge"] = None
                edge["latency_cycles"] = 0
            elif dep_type == "protocol":
                edge["clock"] = clock_logical
                edge["edge"] = clock_edge if clock_edge else None
                edge["latency_cycles"] = latency
            else:  # combinational
                edge["clock"] = None
                edge["edge"] = None
                edge["latency_cycles"] = 0

            # check 字段
            if dep_type == "sequential" and inferred_by == "FF":
                edge["check"] = "="
            elif dep_type == "control" and inferred_by == "FF_CE":
                condition_expr = e.get("condition_expression")
                if condition_expr:
                    edge["condition_expression"] = condition_expr
                    edge["check"] = derive_check_from_condition(condition_expr)
                else:
                    edge["check"] = ">0"
            elif dep_type == "control" and inferred_by == "FF_RST":
                edge["check"] = None

            # description 字段（从 details 提取）
            if details:
                edge["description"] = details

            # 清理 edge 字典
            edge = self._clean_edge(edge, dep_type)

            self.output_edges[canonical_target].append(edge)

            # 记录 signal alias
            self.signal_aliases.setdefault(
                canonical_source,
                canonical_to_modelsim(canonical_source, self._top_module),
            )
            self.signal_aliases.setdefault(
                canonical_target,
                canonical_to_modelsim(canonical_target, self._top_module),
            )

    def _clean_edge(self, edge: dict, dep_type: str) -> dict:
        """清理 edge 字典，严格按 DEPS_FORMAT.md 保留该类型需要的字段。

        DEPS_FORMAT.md 要求的字段契约：
        - 必填: signal, type
        - sequential/memory: clock, edge, latency_cycles, check(可选), description(可选)
        - control: clock(可选), edge(可选), latency_cycles(可选), check(可选), description(可选)
        - boundary: boundary_kind, clock=null, edge=null, latency_cycles=0, check=null
        - combinational: clock=null, edge=null, latency_cycles=0
        - protocol: clock(可选), edge(可选), latency_cycles(可选), protocol_kind(可选)
        """
        result = {}
        # 必填
        result["signal"] = edge["signal"]
        result["type"] = edge["type"]

        if dep_type == "sequential":
            # 时序边：必须保留 clock, edge, latency_cycles
            result["clock"] = edge.get("clock")
            result["edge"] = edge.get("edge", "posedge")
            result["latency_cycles"] = edge.get("latency_cycles", 1)
            if edge.get("check"):
                result["check"] = edge["check"]
            # description 放在最后
            if edge.get("description"):
                result["description"] = edge["description"]

        elif dep_type == "memory":
            # 存储器边：同 sequential
            result["clock"] = edge.get("clock")
            result["edge"] = edge.get("edge", "posedge")
            result["latency_cycles"] = edge.get("latency_cycles", 2)
            if edge.get("check"):
                result["check"] = edge["check"]
            if edge.get("description"):
                result["description"] = edge["description"]

        elif dep_type == "control":
            # 控制边：clock/edge/latency 可选，但应显式输出
            result["clock"] = edge.get("clock")
            result["edge"] = edge.get("edge")
            result["latency_cycles"] = edge.get("latency_cycles", 0)
            if edge.get("check"):
                result["check"] = edge["check"]
            if edge.get("condition_expression"):
                result["condition_expression"] = edge["condition_expression"]
            if edge.get("description"):
                result["description"] = edge["description"]

        elif dep_type == "boundary":
            # 边界节点：clock/edge/latency/check 必须为 null
            result["boundary_kind"] = edge.get("boundary_kind", "input_port")
            result["clock"] = None
            result["edge"] = None
            result["latency_cycles"] = 0
            result["check"] = None

        elif dep_type == "protocol":
            # 协议边
            result["clock"] = edge.get("clock")
            result["edge"] = edge.get("edge")
            result["latency_cycles"] = edge.get("latency_cycles", 0)
            if edge.get("protocol_kind"):
                result["protocol_kind"] = edge["protocol_kind"]
            if edge.get("description"):
                result["description"] = edge["description"]

        else:  # combinational
            # 组合边：clock/edge/latency 必须为 null
            result["clock"] = None
            result["edge"] = None
            result["latency_cycles"] = 0

        return result

    # --------------------------------------------------------
    # 4. 合并 annotations
    # --------------------------------------------------------
    def _apply_annotations(self):
        ann = self.annotations

        # signal_overrides（正则替换）
        for override in ann.get("signal_overrides", []):
            pattern = override.get("vivado_pattern", "")
            template = override.get("canonical_template", "")
            if not pattern or not template:
                continue
            compiled = re.compile(pattern)
            new_edges = {}
            for target, edges in self.output_edges.items():
                new_target = compiled.sub(
                    lambda m: template.format(*m.groups()), target
                )
                for edge in edges:
                    edge["signal"] = compiled.sub(
                        lambda m: template.format(*m.groups()), edge["signal"]
                    )
                if new_target in new_edges:
                    new_edges[new_target].extend(edges)
                    print(f"Warning: signal_overrides collision on '{new_target}'. Edges merged.", file=sys.stderr)
                else:
                    new_edges[new_target] = edges
            self.output_edges = new_edges

        # cdc_boundaries
        for cdc in ann.get("cdc_boundaries", []):
            from_clk = cdc.get("from_clock")
            to_clk = cdc.get("to_clock")
            sync_stages = cdc.get("sync_stages", 2)
            for sig in cdc.get("signals", []):
                canonical = self._canonicalize(sig)
                edge = {
                    "signal": canonical,
                    "type": "boundary",
                    "boundary_kind": "cdc",
                    "clock": to_clk,
                    "edge": "posedge",
                    "latency_cycles": sync_stages,
                    "check": None,
                    "cdc_from_clock": from_clk,
                    "cdc_to_clock": to_clk,
                    "description": f"CDC boundary from {from_clk or '?'} to {to_clk or '?'}",
                }
                self.output_edges.setdefault(canonical, []).append(edge)

        # blackbox_modules
        for bb in ann.get("blackbox_modules", []):
            instance = bb.get("instance", "")
            canonical = self._canonicalize(instance)
            edge = {
                "signal": canonical,
                "type": "boundary",
                "boundary_kind": bb.get("boundary_kind", "blackbox"),
                "clock": None,
                "edge": None,
                "latency_cycles": 0,
                "check": None,
                "description": bb.get("description", ""),
            }
            self.output_edges.setdefault(canonical, []).append(edge)

        # latency_overrides
        # signal 字段指定目标输出信号，对其所有上游依赖边应用新延迟
        for lo in ann.get("latency_overrides", []):
            sig = lo.get("signal", "")
            canonical = self._canonicalize(sig)
            new_latency = lo.get("latency_cycles")
            new_clock = lo.get("clock")
            if new_latency is not None:
                # 匹配目标输出节点（output key）
                if canonical in self.output_edges:
                    for edge in self.output_edges[canonical]:
                        edge["latency_cycles"] = new_latency
                        if new_clock:
                            edge["clock"] = new_clock

        # constants — fixed-value signals as boundary nodes
        for const in ann.get("constants", []):
            sig = const.get("signal", "")
            canonical = self._canonicalize(sig)
            value = const.get("value", "")
            desc = const.get("description", "")
            edge = {
                "signal": canonical,
                "type": "boundary",
                "boundary_kind": "constant",
                "clock": None,
                "edge": None,
                "latency_cycles": 0,
                "check": None,
                "description": f"Constant: {value}" + (f" — {desc}" if desc else ""),
            }
            self.output_edges.setdefault(canonical, []).append(edge)
            self.signal_aliases[canonical] = sig

    # --------------------------------------------------------
    # 5. signal_aliases
    # --------------------------------------------------------
    def _build_signal_aliases(self):
        # Phase 1: Apply signal_overrides regex to find unmatched Vivado paths
        # that need alias entries beyond what was accumulated during edge conversion.
        for override in self.annotations.get("signal_overrides", []):
            pattern = override.get("vivado_pattern", "")
            template = override.get("canonical_template", "")
            if not pattern or not template:
                continue
            compiled = re.compile(pattern)
            # Scan all Vivado paths from raw data to find ones not yet aliased
            for source_set in [self.raw.get("boundary_ports", []), self.raw.get("edges", [])]:
                for item in source_set:
                    for key in ("path", "source", "target"):
                        vivado_path = item.get(key, "")
                        if not vivado_path:
                            continue
                        canonical = compiled.sub(
                            lambda m: template.format(*m.groups()), vivado_path
                        )
                        # Only add if canonical differs from vivado_path AND
                        # not already in aliases (either direction)
                        if canonical not in self.signal_aliases:
                            self.signal_aliases[canonical] = canonical_to_modelsim(canonical, self._top_module)
                        if canonical != vivado_path and vivado_path not in self.signal_aliases:
                            self.signal_aliases[vivado_path] = canonical_to_modelsim(canonical, self._top_module)

        # Phase 2: Ensure all boundary_ports have alias entries
        for bp in self.raw.get("boundary_ports", []):
            path = bp.get("path", "")
            if not path:
                continue
            canonical = self._canonicalize(path)
            if canonical not in self.signal_aliases:
                self.signal_aliases[canonical] = canonical_to_modelsim(canonical, self._top_module)

        # Phase 3: Ensure all edge source/target signals have alias entries
        for e in self.raw.get("edges", []):
            for key in ("source", "target"):
                raw_path = e.get(key, "")
                if not raw_path:
                    continue
                canonical = self._canonicalize(raw_path)
                if canonical not in self.signal_aliases:
                    self.signal_aliases[canonical] = canonical_to_modelsim(canonical, self._top_module)

    # --------------------------------------------------------
    # 6. infer categories
    # --------------------------------------------------------
    def _infer_categories(self):
        for target, edges in self.output_edges.items():
            if target in self.categories:
                continue
            # 从 edges 的类型推断
            types = {e.get("type") for e in edges}
            if "memory" in types:
                self.categories[target] = "memory"
            elif "protocol" in types:
                self.categories[target] = "protocol"
            elif "sequential" in types:
                self.categories[target] = infer_category(target, "sequential")
            else:
                self.categories[target] = infer_category(target, "combinational")

    # --------------------------------------------------------
    # 7. 组装输出
    # --------------------------------------------------------
    def _assemble_output(self) -> dict:
        # 组装 dependencies
        dependencies = []
        for output, edges in sorted(self.output_edges.items()):
            entry = {
                "output": output,
                "category": self.categories.get(output, "data"),
                "depends_on": edges,
            }
            dependencies.append(entry)

        # 组装 signal_aliases
        signal_alias_list = []
        for canonical, modelsim_path in sorted(self.signal_aliases.items()):
            if canonical != modelsim_path:  # 只有需要别名映射的才加
                signal_alias_list.append({
                    "canonical": canonical,
                    "modelsim": modelsim_path,
                })

        top_module = self.raw.get("top_module", "unknown")
        extractor = self.raw.get("extractor", "unknown")

        return {
            "format_version": "1.0",
            "description": f"Auto-generated from {extractor} extraction of {top_module}",
            "signal_aliases": signal_alias_list if signal_alias_list else [],
            "clock_aliases": self.clock_aliases,
            "dependencies": dependencies,
        }


# ============================================================
# 增量对比
# ============================================================

def compute_diff(old_deps: dict, new_deps: dict) -> list[str]:
    """比较两个 deps.yaml，输出变更报告。"""
    changes = []

    old_edges = {}
    for entry in old_deps.get("dependencies", []):
        output = entry["output"]
        for edge in entry.get("depends_on", []):
            key = (output, edge.get("signal"), edge.get("type"))
            old_edges[key] = edge

    new_edges = {}
    for entry in new_deps.get("dependencies", []):
        output = entry["output"]
        for edge in entry.get("depends_on", []):
            key = (output, edge.get("signal"), edge.get("type"))
            new_edges[key] = edge

    # Added
    for key in sorted(new_edges.keys() - old_edges.keys()):
        output, signal, dep_type = key
        changes.append(f"+ Added: {signal} → {output} [{dep_type}]")

    # Removed
    for key in sorted(old_edges.keys() - new_edges.keys()):
        output, signal, dep_type = key
        changes.append(f"- Removed: {signal} → {output} [{dep_type}]")

    # Changed (latency, clock, etc.)
    for key in sorted(old_edges.keys() & new_edges.keys()):
        old_e = old_edges[key]
        new_e = new_edges[key]
        diffs = []
        if old_e.get("latency_cycles") != new_e.get("latency_cycles"):
            diffs.append(
                f"latency_cycles: {old_e.get('latency_cycles')} → {new_e.get('latency_cycles')}"
            )
        if old_e.get("clock") != new_e.get("clock"):
            diffs.append(f"clock: {old_e.get('clock')} → {new_e.get('clock')}")
        if diffs:
            output, signal, dep_type = key
            changes.append(f"~ Changed: {signal} → {output} [{dep_type}] ({', '.join(diffs)})")

    return changes


# ============================================================
# 主入口
# ============================================================

def main():
    parser = argparse.ArgumentParser(
        description="Convert deps_raw.json to deps.yaml format"
    )
    parser.add_argument(
        "input", help="Path to deps_raw.json (from Vivado Tcl or Pyverilog)"
    )
    parser.add_argument(
        "-o", "--output", default="deps.yaml", help="Output file path (default: deps.yaml)"
    )
    parser.add_argument(
        "--annotate", help="Path to annotations.yaml for manual overrides"
    )
    parser.add_argument(
        "--format", choices=["yaml", "json"], default="yaml", help="Output format (default: yaml)"
    )
    parser.add_argument(
        "--diff", help="Path to existing deps.yaml for diff report"
    )

    args = parser.parse_args()

    # 读取输入
    with open(args.input, "r", encoding="utf-8") as f:
        raw_data = json.load(f)

    # 读取 annotations（可选）
    annotations = None
    if args.annotate:
        with open(args.annotate, "r", encoding="utf-8") as f:
            annotations = yaml.safe_load(f)

    # 转换
    converter = DepsConverter(raw_data, annotations)
    try:
        result = converter.convert()
    except ValueError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

    # 增量对比（可选）
    if args.diff:
        with open(args.diff, "r", encoding="utf-8") as f:
            old_deps = yaml.safe_load(f)
        changes = compute_diff(old_deps, result)
        if changes:
            print("Dependency changes:")
            for c in changes:
                print(f"  {c}")
        else:
            print("No changes detected.")

    # 输出
    if args.format == "yaml":
        with open(args.output, "w", encoding="utf-8") as f:
            yaml.dump(result, f, default_flow_style=False, allow_unicode=True, sort_keys=False)
        print(f"Written: {args.output}")
    else:
        with open(args.output, "w", encoding="utf-8") as f:
            json.dump(result, f, indent=2, ensure_ascii=False)
        print(f"Written: {args.output}")


if __name__ == "__main__":
    main()
