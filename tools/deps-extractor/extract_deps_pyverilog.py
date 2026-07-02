#!/usr/bin/env python3
"""extract_deps_pyverilog.py — Pyverilog dataflow 提取：Verilog 源码 → deps_raw.json

用法:
    python extract_deps_pyverilog.py rtl_file1.v rtl_file2.v ... -t top_module -o deps_raw.json

说明:
    - 纯源码级分析，无需打开 Vivado
    - 适用于 Verilog-2001 设计，无 generate/复杂参数化
    - 输出复用与 Vivado Tcl 相同的 deps_raw.json 中间格式
    - inferred_type 初始为 "unknown"，由 deps_converter.py 进一步推断

局限:
    - 不区分时序/组合依赖（通过 always 块分析弥补）
    - 不支持 generate 展开
    - 不支持 SV interface/package
    - 无 BRAM/时钟推断（由后处理脚本按信号名规则推断）
"""

import argparse
import json
import re
import sys
import os
import tempfile
import shutil
from datetime import datetime, timezone
from pathlib import Path

try:
    from pyverilog.dataflow.dataflow_analyzer import VerilogDataflowAnalyzer
    import pyverilog.vparser.ast as ast
    from pyverilog.vparser.parser import parse as pyv_parse
except ImportError:
    print(
        "Error: Pyverilog is required. Install with: pip install pyverilog",
        file=sys.stderr,
    )
    sys.exit(1)


# ============================================================
# SystemVerilog → Verilog-2001 预处理器
# ============================================================

def normalize_sv_to_v2001(source: str) -> str:
    """将常见 SystemVerilog 语法降级为 Verilog-2001，使 pyverilog 可解析。

    处理范围：
    - always_ff / always_comb / always_latch → always
    - logic / bit → reg
    - typedef enum / typedef struct → 注释掉
    - import package::* → 注释掉
    - interface ... endinterface → 注释掉
    - $clog2() / $bits() → 近似常量
    - unique / priority 关键字 → 移除
    - assert property → 注释掉
    - string → reg [255:0]
    """
    text = source

    # ── Multi-line block removals (before single-line passes) ──

    # typedef enum ... end_type / typedef struct ... end_type
    # Match typedef (enum|struct) { ... } name;
    text = re.sub(
        r'typedef\s+(?:enum|struct)\s*(?:\w+\s*)?\{[^}]*\}\s*\w+\s*;',
        lambda m: '// ' + m.group(0).replace('\n', '\n// '),
        text,
        flags=re.DOTALL,
    )
    # Also handle typedef enum without braces (simple enum with type)
    text = re.sub(
        r'typedef\s+enum\s*(?:\w+\s*)?(?:<[^>]*>)?\s*\{[^}]*\}\s*\w+\s*;',
        lambda m: '// ' + m.group(0).replace('\n', '\n// '),
        text,
        flags=re.DOTALL,
    )

    # interface ... endinterface (entire blocks)
    text = re.sub(
        r'^(\s*)interface\b.*?^(\s*)endinterface\b',
        lambda m: '\n'.join('// ' + line for line in m.group(0).split('\n')),
        text,
        flags=re.DOTALL | re.MULTILINE,
    )

    # ── Single-line transformations ──

    # always_ff @(posedge clk) → always @(posedge clk)
    text = re.sub(r'\balways_ff\b', 'always  ', text)
    # always_comb → always @(*)
    text = re.sub(r'\balways_comb\b', 'always @(*)', text)
    # always_latch → always @(*)
    text = re.sub(r'\balways_latch\b', 'always @(*)', text)

    # import package::* → comment out
    text = re.sub(r'^(\s*)import\s+\w+::\*;', r'\1// import package::*;', text, flags=re.MULTILINE)
    text = re.sub(r'^(\s*)import\s+\w+::\w+;', r'\1// import package::item;', text, flags=re.MULTILINE)

    # logic type — context-aware replacement:
    #   input logic  → input wire   (input ports must be wire in V2001)
    #   inout logic  → inout wire
    #   output logic → output reg
    #   standalone   → reg          (internal signals default to reg)
    text = re.sub(r'\binput\s+logic\b', 'input  wire', text)
    text = re.sub(r'\binout\s+logic\b', 'inout  wire', text)
    text = re.sub(r'\boutput\s+logic\b', 'output reg ', text)
    text = re.sub(r'\blogic\b', 'reg  ', text)

    # bit type — same context-aware rules as logic
    text = re.sub(r'\binput\s+bit\b', 'input  wire', text)
    text = re.sub(r'\binout\s+bit\b', 'inout  wire', text)
    text = re.sub(r'\boutput\s+bit\b', 'output reg ', text)
    text = re.sub(r'\bbit\b(?!\s*[\'"])', 'reg', text)

    # string type → reg [255:0]
    text = re.sub(r'\bstring\b', 'reg [255:0] /*string*/', text)

    # unique / priority before case/if → remove
    text = re.sub(r'\bunique\s+(?=case|if)', '', text)
    text = re.sub(r'\bpriority\s+(?=case|if)', '', text)

    # assert property(...) → comment out entire line
    text = re.sub(r'^(\s*)assert\s+property\b', r'\1// assert property', text, flags=re.MULTILINE)
    text = re.sub(r'^(\s*)assert\s*\(', r'\1// assert(', text, flags=re.MULTILINE)

    # $clog2(expr) → 32 (approximate, avoids parse failure)
    text = re.sub(r'\$clog2\s*\([^)]*\)', '32', text)
    # $bits(expr) → 32
    text = re.sub(r'\$bits\s*\([^)]*\)', '32', text)
    # $size(expr) → 32
    text = re.sub(r'\$size\s*\([^)]*\)', '32', text)

    # automatic keyword in function/task → remove
    text = re.sub(r'\bautomatic\s+', '', text)

    # void function / void task → just function/task
    text = re.sub(r'\bvoid\s+(?=function|task)', '', text)

    # endfunction → endfunction (already V2001 compatible, keep)
    # endtask → endtask (already V2001 compatible, keep)

    # Remove `pragma / `line directives that may confuse parser
    text = re.sub(r'^\s*`pragma\b.*$', '', text, flags=re.MULTILINE)

    return text


# ============================================================
# AST 分析：always 块识别
# ============================================================

class AlwaysBlockAnalyzer:
    """遍历 AST，识别 always @(posedge clk) 块中的寄存器赋值。

    输出：
    - sequential_assigns: {dest: {clock, clock_edge, sources, ce_signals}}
    - reset_assigns: {dest: (reset_signal, reset_value_signals)}
    - combinational_assigns: {dest: [sources]}
    - clocks: [(logical_name, period_ns)]
    - ports: [(name, direction, width)]
    """

    def __init__(self):
        self.sequential_assigns: dict = {}
        self.reset_assigns: dict = {}
        self.combinational_assigns: dict = {}  # {dest: [sources]}
        self.clocks: list = []  # [(logical_name, period_ns)]
        self.ports: list = []  # [(name, direction, width)]

    def visit(self, node):
        """通用 visit 方法：分发到具体 visit_Xxx 方法。"""
        if node is None:
            return
        method_name = f"visit_{type(node).__name__}"
        visitor = getattr(self, method_name, None)
        if visitor:
            visitor(node)

    def visit_SourceDescription(self, node):
        """遍历顶层描述节点。"""
        if hasattr(node, 'definitions'):
            for item in node.definitions:
                self.visit(item)

    def visit_Source(self, node):
        """处理 vparser 顶层 Source 节点。"""
        if hasattr(node, 'description'):
            self.visit(node.description)

    def visit_Description(self, node):
        """处理 Description 节点。"""
        if hasattr(node, 'definitions'):
            for item in node.definitions:
                self.visit(item)

    def visit_ModuleDef(self, node):
        """提取模块端口列表。

        vparser AST:
        - node.portlist.ports: list of Ioport
        - Ioport.first: Input/Output/Inout with name and width
        """
        if node.portlist:
            for ioport in node.portlist.ports:
                if hasattr(ioport, 'first'):
                    port_decl = ioport.first
                    name = getattr(port_decl, 'name', None)
                    if not name:
                        continue
                    if isinstance(port_decl, ast.Input):
                        width = self._get_width(port_decl)
                        self.ports.append((name, "IN", width))
                    elif isinstance(port_decl, ast.Output):
                        width = self._get_width(port_decl)
                        self.ports.append((name, "OUT", width))
                    elif isinstance(port_decl, ast.Inout):
                        width = self._get_width(port_decl)
                        self.ports.append((name, "INOUT", width))

        # 继续遍历内部项
        for item in node.items:
            self.visit(item)

    def visit_Always(self, node):
        """分析 always 块。

        pyverilog.vparser AST:
        - node.sens_list: SensList with .list attribute
        - node.statement: 语句体
        """
        sens_list = node.sens_list.list if hasattr(node.sens_list, 'list') else []
        if not sens_list:
            return

        # 判断是否是时序 always 块
        is_sequential = False
        clock_signal = None
        clock_edge = "posedge"

        for sensor in sens_list:
            if isinstance(sensor, ast.Sens):
                sig_name = self._get_signal_name(sensor.sig) if hasattr(sensor, 'sig') else None
                is_reset_signal = bool(sig_name and re.search(r'(?i)(\brst(?:_|$)|\breset(?:_|$)|\barst(?:_|$))', sig_name))
                if sensor.type == "posedge":
                    if is_reset_signal:
                        # Reset is not a clock alias; keep the real clock from the same block.
                        continue
                    is_sequential = True
                    clock_signal = sig_name
                    clock_edge = "posedge"
                    if clock_signal:
                        self.clocks.append((clock_signal, 0.0))
                elif sensor.type == "negedge":
                    # 检查是否是复位信号（不是时钟）
                    if is_reset_signal:
                        # 复位信号，不作为时钟记录
                        pass
                    else:
                        is_sequential = True
                        clock_signal = sig_name
                        clock_edge = "negedge"
                        if clock_signal:
                            self.clocks.append((clock_signal, 0.0))
                elif sensor.type == "level" and not sig_name:
                    # 组合逻辑敏感列表（如 @* 或 @(*)）
                    pass

        if is_sequential and clock_signal:
            # 遍历 always 块内的赋值
            self._analyze_sequential_block(node.statement, clock_signal, clock_edge)
        else:
            # 组合逻辑 always 块
            self._analyze_combinational_block(node.statement)

    def _analyze_sequential_block(self, stmt, clock, edge, ce_signals=None, guard_signals=None, ce_condition_expr=None):
        """分析时序 always 块内的赋值。

        处理 if-else 结构：
        if (!rst_n) data_o <= 0;      → reset
        else if (enable) data_o <= data_i;  → sequential + control
        else if (cond) count <= count + 1;  → sequential + guard deps

        ce_signals: 最外层使能信号（如 enable），作为 FF_CE 边
        guard_signals: 嵌套 if 条件中的数据依赖（如 threshold from count < threshold）
        """
        if ce_signals is None:
            ce_signals = []
        if guard_signals is None:
            guard_signals = []

        if stmt is None:
            return

        if isinstance(stmt, ast.Block):
            for s in stmt.statements:
                self._analyze_sequential_block(s, clock, edge, ce_signals, guard_signals, ce_condition_expr)
            return

        if isinstance(stmt, ast.IfStatement):
            # 检查是否是复位条件
            cond = stmt.cond
            cond_signals = self._extract_signals(cond)

            # 判断复位（!rst_n 或 rst_n == 0）
            is_reset = False
            reset_sig = None
            for sig in cond_signals:
                if re.search(r'(?i)(\brst(?:_|$)|\breset(?:_|$)|\barst(?:_|$))', sig):
                    is_reset = True
                    reset_sig = sig
                    break

            if is_reset:
                # 复位分支
                self._collect_reset_assignments(stmt.true_statement, reset_sig)
                # else 分支
                if isinstance(stmt.false_statement, ast.IfStatement):
                    self._analyze_sequential_block(stmt.false_statement, clock, edge, ce_signals, guard_signals)
                elif isinstance(stmt.false_statement, ast.CaseStatement):
                    self._analyze_case_sequential(stmt.false_statement, clock, edge)
                elif isinstance(stmt.false_statement, ast.Block):
                    for s in stmt.false_statement.statements:
                        self._analyze_sequential_block(s, clock, edge, ce_signals, guard_signals)
                else:
                    self._analyze_sequential_block(
                        stmt.false_statement,
                        clock,
                        edge,
                        ce_signals,
                        guard_signals,
                    )
            else:
                # 使能/守卫条件
                ce_expr = self._extract_condition_expression(cond)

                # 区分 CE 信号和 guard 信号：
                # - CE: 单个简单信号条件（如 if (enable)），用作 FF_CE 边
                # - Guard: 复合条件或多信号条件（如 if (state == S_RUN && count < threshold)），
                #   作为数据依赖合并到 sources
                is_simple_ce = (len(cond_signals) == 1 and ce_expr is not None
                                and not any(op in ce_expr for op in ['==', '!=', '<', '>', '<=', '>=']))

                if not ce_signals and not guard_signals:
                    # 第一个非 reset 条件
                    if is_simple_ce:
                        current_ce = cond_signals
                        current_guard = []
                    else:
                        current_ce = []
                        current_guard = [s for s in cond_signals if s not in (guard_signals or [])]
                elif ce_signals:
                    # 已有 CE，后续嵌套条件 = guard
                    current_ce = ce_signals
                    current_guard = list(guard_signals) + [s for s in cond_signals if s not in ce_signals]
                else:
                    # 已有 guard，继续累积
                    current_ce = []
                    current_guard = list(guard_signals) + [s for s in cond_signals if s not in (guard_signals or [])]

                if isinstance(stmt.true_statement, ast.IfStatement):
                    self._analyze_sequential_block(stmt.true_statement, clock, edge, current_ce, current_guard, ce_expr)
                elif isinstance(stmt.true_statement, ast.CaseStatement):
                    self._analyze_case_sequential(stmt.true_statement, clock, edge)
                else:
                    self._analyze_sequential_block(
                        stmt.true_statement,
                        clock,
                        edge,
                        current_ce,
                        current_guard,
                        ce_expr,
                    )
                if isinstance(stmt.false_statement, ast.IfStatement):
                    self._analyze_sequential_block(stmt.false_statement, clock, edge, current_ce, current_guard)
                elif isinstance(stmt.false_statement, ast.CaseStatement):
                    self._analyze_case_sequential(stmt.false_statement, clock, edge)
                else:
                    self._analyze_sequential_block(
                        stmt.false_statement,
                        clock,
                        edge,
                        [],
                        current_guard,
                    )

        elif isinstance(stmt, ast.NonblockingSubstitution):
            # 直接的非阻塞赋值（无条件）
            self._analyze_assignment(stmt, clock, edge, ce_signals, ce_condition_expr, guard_signals)

        elif isinstance(stmt, ast.BlockingSubstitution):
            # 阻塞赋值（在时序块中也视为数据流依赖）
            self._analyze_assignment(stmt, clock, edge, ce_signals, ce_condition_expr, guard_signals)

        elif isinstance(stmt, ast.CaseStatement):
            # case/casex/casez 语句（如 FSM next-state 逻辑）
            self._analyze_case_sequential(stmt, clock, edge)

        elif isinstance(stmt, ast.Block):
            # 语句块（在 vparser 中是 ast.Block）
            for s in stmt.statements:
                self._analyze_sequential_block(s, clock, edge, ce_signals, guard_signals, ce_condition_expr)

    def _collect_reset_assignments(self, stmt, reset_sig):
        if stmt is None:
            return
        if isinstance(stmt, ast.Block):
            for s in stmt.statements:
                self._collect_reset_assignments(s, reset_sig)
            return
        if isinstance(stmt, ast.IfStatement):
            self._collect_reset_assignments(stmt.true_statement, reset_sig)
            self._collect_reset_assignments(stmt.false_statement, reset_sig)
            return
        if isinstance(stmt, ast.CaseStatement):
            for item in (stmt.caselist or []):
                if hasattr(item, "statement"):
                    self._collect_reset_assignments(item.statement, reset_sig)
            return
        if isinstance(stmt, (ast.NonblockingSubstitution, ast.BlockingSubstitution)):
            dest = self._get_signal_name(stmt.left)
            if dest:
                src_signals = self._extract_signals(stmt.right)
                self.reset_assigns[dest] = (reset_sig, src_signals)

    def _analyze_assignment(self, stmt, clock, edge, ce_signals, ce_condition_expr=None, guard_signals=None):
        """分析单个赋值（非阻塞或阻塞）。

        ce_signals: if/else 条件中的使能信号（如 enable），作为 FF_CE 边
        guard_signals: 嵌套 if 条件中的数据依赖信号（如 threshold from count < threshold），
                      合并到 sources 但不产生 CE 边
        """
        if not isinstance(stmt, (ast.NonblockingSubstitution, ast.BlockingSubstitution)):
            return

        dest = self._get_signal_name(stmt.left)
        if not dest:
            return

        src_signals = list(self._extract_signals(stmt.right))

        # 合并 guard 信号到 sources（如 threshold from count < threshold）
        # 排除 ce_signals（已作为控制边处理）和 dest 自身
        ce_set = set(ce_signals)
        if guard_signals:
            for gs in guard_signals:
                if gs and gs != dest and gs not in src_signals and gs not in ce_set:
                    src_signals.append(gs)

        self.sequential_assigns[dest] = {
            "clock": clock,
            "clock_edge": edge,
            "sources": src_signals,
            "ce_signals": ce_signals,
            "condition_expression": ce_condition_expr,
        }

    def _analyze_case_sequential(self, node, clock, edge):
        """分析时序 always 块内的 case/casex/casez 语句。

        case 语句中的赋值被视为 sequential，比较表达式和条件值作为 CE 信号。
        """
        comp_signals = self._extract_signals(node.comp) if hasattr(node, 'comp') and node.comp else []
        comp_expr = self._extract_condition_expression(node.comp) if hasattr(node, 'comp') and node.comp else None

        for item in (node.caselist or []):
            # 提取条件中的信号（case 常量通常不含信号，但可能包含参数）
            cond_signals = []
            ce_expr = None
            if hasattr(item, 'cond') and item.cond:
                for cond_expr_node in item.cond:
                    if cond_expr_node is not None:
                        cond_signals.extend(self._extract_signals(cond_expr_node))

                # 尝试重建条件表达式: comp_expr == case_value
                if comp_expr and len(item.cond) == 1 and item.cond[0] is not None:
                    case_val_expr = self._extract_condition_expression(item.cond[0])
                    if case_val_expr:
                        ce_expr = f"({comp_expr} == {case_val_expr})"

            # CE 信号 = 比较表达式信号 + 条件信号
            ce_signals = list(set(comp_signals + cond_signals))

            # 处理 case body
            if hasattr(item, 'statement') and item.statement:
                body = item.statement
                if isinstance(body, (ast.NonblockingSubstitution, ast.BlockingSubstitution)):
                    dest = self._get_signal_name(body.left)
                    if dest:
                        src_signals = self._extract_signals(body.right)
                        self.sequential_assigns[dest] = {
                            "clock": clock,
                            "clock_edge": edge,
                            "sources": src_signals,
                            "ce_signals": ce_signals,
                            "condition_expression": ce_expr,
                        }
                elif isinstance(body, ast.Block):
                    for s in body.statements:
                        self._analyze_sequential_block(s, clock, edge)
                elif isinstance(body, ast.IfStatement):
                    self._analyze_sequential_block(body, clock, edge)
                elif isinstance(body, ast.CaseStatement):
                    self._analyze_case_sequential(body, clock, edge)

    def _analyze_combinational_block(self, stmt, comp_signals=None):
        """分析组合逻辑 always 块。

        comp_signals: 从外层 case 语句传播下来的比较信号（如 state）。
        """
        extra = comp_signals or []
        if isinstance(stmt, (ast.NonblockingSubstitution, ast.BlockingSubstitution)):
            dest = self._get_signal_name(stmt.left)
            if dest:
                src_signals = self._extract_signals(stmt.right)
                self.combinational_assigns.setdefault(dest, []).extend(src_signals + extra)
        elif isinstance(stmt, ast.Block):
            for s in stmt.statements:
                self._analyze_combinational_block(s, comp_signals)
        elif isinstance(stmt, ast.CaseStatement):
            self._analyze_case_combinational(stmt)
        elif isinstance(stmt, ast.IfStatement):
            cond_signals = self._extract_signals(stmt.cond)
            all_extra = extra + cond_signals
            if isinstance(stmt.true_statement, (ast.NonblockingSubstitution, ast.BlockingSubstitution)):
                dest = self._get_signal_name(stmt.true_statement.left)
                if dest:
                    src_signals = self._extract_signals(stmt.true_statement.right)
                    self.combinational_assigns.setdefault(dest, []).extend(src_signals + all_extra)
            elif isinstance(stmt.true_statement, ast.CaseStatement):
                self._analyze_case_combinational(stmt.true_statement)
            elif isinstance(stmt.true_statement, ast.Block):
                for s in stmt.true_statement.statements:
                    self._analyze_combinational_block(s, all_extra)
            if stmt.false_statement:
                self._analyze_combinational_block(stmt.false_statement, all_extra)

    def _analyze_case_combinational(self, node):
        """分析组合 always 块内的 case/casex/casez 语句。

        case 语句中的赋值被视为 combinational，比较表达式信号作为依赖。
        case 比较信号（如 state）传播到每个分支内的赋值。
        """
        comp_signals = self._extract_signals(node.comp) if hasattr(node, 'comp') and node.comp else []

        for item in (node.caselist or []):
            if hasattr(item, 'cond') and item.cond:
                for cond_expr in item.cond:
                    if cond_expr is not None:
                        comp_signals.extend(self._extract_signals(cond_expr))

            if hasattr(item, 'statement') and item.statement:
                body = item.statement
                if isinstance(body, (ast.NonblockingSubstitution, ast.BlockingSubstitution)):
                    dest = self._get_signal_name(body.left)
                    if dest:
                        src_signals = self._extract_signals(body.right)
                        self.combinational_assigns.setdefault(dest, []).extend(src_signals + comp_signals)
                elif isinstance(body, ast.Block):
                    for s in body.statements:
                        self._analyze_combinational_block(s, comp_signals)
                elif isinstance(body, ast.IfStatement):
                    self._analyze_combinational_block(body, comp_signals)
                elif isinstance(body, ast.CaseStatement):
                    self._analyze_case_combinational(body)

    def visit_Assign(self, node):
        """处理连续赋值 assign 语句（组合逻辑）。"""
        dest = self._get_signal_name(node.left)
        if dest:
            src_signals = self._extract_signals(node.right)
            self.combinational_assigns.setdefault(dest, []).extend(src_signals)

    # ============================================================
    # 辅助方法
    # ============================================================

    def _get_signal_name(self, node) -> str | None:
        """从 AST 节点提取信号名。"""
        if node is None:
            return None
        if isinstance(node, ast.Identifier):
            return node.name
        if isinstance(node, ast.Lvalue):
            # Lvalue wraps the actual variable
            return self._get_signal_name(node.var) if hasattr(node, 'var') else None
        if isinstance(node, ast.Rvalue):
            return self._get_signal_name(node.var) if hasattr(node, 'var') else None
        if isinstance(node, ast.Pointer):
            base = self._get_signal_name(node.var)
            return base
        if isinstance(node, ast.Partselect):
            base = self._get_signal_name(node.var)
            return base
        if isinstance(node, ast.UnaryOperator):
            return self._get_signal_name(node.right)
        return None

    def _extract_condition_expression(self, node) -> str | None:
        """从 AST 条件节点重建可被 LALRPOP 解析的条件字符串。

        返回 None 表示含不支持运算符，下游回退到 check 字段。
        信号名使用 canonical 格式 (TOP.xxx)，与 deps_raw.json 一致。
        """
        if node is None:
            return None

        if isinstance(node, ast.Identifier):
            return f"TOP.{node.name}"

        if isinstance(node, ast.Rvalue):
            if hasattr(node, 'var'):
                return self._extract_condition_expression(node.var)
            return None

        if isinstance(node, ast.Lvalue):
            if hasattr(node, 'var'):
                return self._extract_condition_expression(node.var)
            return None

        if isinstance(node, ast.IntConst):
            return self._format_int_const(node)

        if isinstance(node, ast.UnaryOperator):
            op = getattr(node, 'operator', None)
            if op == '!':
                inner = self._extract_condition_expression(node.right)
                if inner is None:
                    return None
                return f"!({inner})"
            elif op == '~':
                inner = self._extract_condition_expression(node.right)
                if inner is None:
                    return None
                return f"~({inner})"
            # 其他 unary 运算符（-、+、&、|、^ 等）不支持
            return None

        # Land/Lor: pyverilog 对 && 和 || 使用独立 AST 节点（继承自 Operator）
        if hasattr(ast, 'Land') and isinstance(node, ast.Land):
            left = self._extract_condition_expression(node.left)
            right = self._extract_condition_expression(node.right)
            if left is None or right is None:
                return None
            return f"({left} && {right})"

        if hasattr(ast, 'Lor') and isinstance(node, ast.Lor):
            left = self._extract_condition_expression(node.left)
            right = self._extract_condition_expression(node.right)
            if left is None or right is None:
                return None
            return f"({left} || {right})"

        # Handle Shift operations (<<, >>) — pyverilog uses separate Lshift/Rshift AST classes
        if hasattr(ast, 'Lshift') and isinstance(node, ast.Lshift):
            left = self._extract_condition_expression(node.left)
            right = self._extract_condition_expression(node.right)
            if left is None or right is None:
                return None
            return f"({left} << {right})"

        if hasattr(ast, 'Rshift') and isinstance(node, ast.Rshift):
            left = self._extract_condition_expression(node.left)
            right = self._extract_condition_expression(node.right)
            if left is None or right is None:
                return None
            return f"({left} >> {right})"

        if isinstance(node, ast.Operator):
            op = getattr(node, 'operator', None)
            supported_ops = {'==', '!=', '&&', '||', '&', '|', '^', '<<', '>>'}
            if op not in supported_ops:
                return None

            # Operator 可能用 args 或 left/right
            if hasattr(node, 'args') and node.args and len(node.args) == 2:
                left = self._extract_condition_expression(node.args[0])
                right = self._extract_condition_expression(node.args[1])
            elif hasattr(node, 'left') and hasattr(node, 'right'):
                left = self._extract_condition_expression(node.left)
                right = self._extract_condition_expression(node.right)
            else:
                return None

            if left is None or right is None:
                return None

            return f"({left} {op} {right})"

        if isinstance(node, ast.Partselect):
            base = self._get_signal_name(node.var)
            if not base:
                return None
            msb = self._eval_const(node.msb) if hasattr(node, 'msb') and node.msb else None
            lsb = self._eval_const(node.lsb) if hasattr(node, 'lsb') and node.lsb else None
            if msb is not None and lsb is not None:
                return f"TOP.{base}[{msb}:{lsb}]"
            # 无法提取常量范围，回退到裸信号
            return f"TOP.{base}"

        if isinstance(node, ast.Pointer):
            base = self._get_signal_name(node.var)
            if not base:
                return None
            idx = self._eval_const(getattr(node, 'index', None))
            if idx is not None:
                return f"TOP.{base}[{idx}]"
            # 动态索引不支持
            return f"TOP.{base}"

        # Cond（三元）、Concat、Repeat 等不支持
        return None

    def _format_int_const(self, node) -> str:
        """将 IntConst 格式化为 Verilog 常量字符串 (LALRPOP 可解析)。

        格式: {width}'d{value} 或原始 Verilog 格式。
        """
        raw = node.value
        # 如果已经是 Verilog 常量格式，直接使用
        if "'" in raw:
            return raw

        # 解析为整数
        try:
            val = int(raw)
        except ValueError:
            return f"1'd0"

        # 计算最小位宽
        width = max(1, val.bit_length()) if val > 0 else 1
        return f"{width}'d{val}"

    def _extract_signals(self, node) -> list[str]:
        """从表达式树中提取所有信号名。"""
        signals = []
        if node is None:
            return signals
        if isinstance(node, ast.Identifier):
            signals.append(node.name)
        elif isinstance(node, ast.Rvalue):
            if hasattr(node, 'var'):
                signals.extend(self._extract_signals(node.var))
        elif isinstance(node, ast.IntConst):
            pass  # 常量，不提取
        elif isinstance(node, ast.Pointer):
            base = self._get_signal_name(node.var)
            if base:
                signals.append(base)
        elif isinstance(node, ast.Partselect):
            base = self._get_signal_name(node.var)
            if base:
                signals.append(base)
        elif isinstance(node, ast.UnaryOperator):
            signals.extend(self._extract_signals(node.right))
        elif hasattr(ast, 'Land') and isinstance(node, ast.Land):
            signals.extend(self._extract_signals(node.left))
            signals.extend(self._extract_signals(node.right))
        elif hasattr(ast, 'Lor') and isinstance(node, ast.Lor):
            signals.extend(self._extract_signals(node.left))
            signals.extend(self._extract_signals(node.right))
        elif hasattr(ast, 'Lshift') and isinstance(node, ast.Lshift):
            signals.extend(self._extract_signals(node.left))
            signals.extend(self._extract_signals(node.right))
        elif hasattr(ast, 'Rshift') and isinstance(node, ast.Rshift):
            signals.extend(self._extract_signals(node.left))
            signals.extend(self._extract_signals(node.right))
        elif isinstance(node, ast.Operator):
            if hasattr(node, 'args'):
                for arg in node.args:
                    signals.extend(self._extract_signals(arg))
            elif hasattr(node, 'left') and hasattr(node, 'right'):
                signals.extend(self._extract_signals(node.left))
                signals.extend(self._extract_signals(node.right))
        elif isinstance(node, ast.Cond):
            signals.extend(self._extract_signals(node.cond))
            signals.extend(self._extract_signals(node.true_value))
            signals.extend(self._extract_signals(node.false_value))
        elif isinstance(node, ast.Concat):
            for item in node.list:
                signals.extend(self._extract_signals(item))
        elif isinstance(node, ast.Repeat):
            signals.extend(self._extract_signals(node.value))
        elif isinstance(node, ast.SystemCall):
            for arg in node.args:
                signals.extend(self._extract_signals(arg))
        elif isinstance(node, ast.Call):
            for arg in node.args:
                signals.extend(self._extract_signals(arg))
        elif isinstance(node, ast.Lvalue):
            if hasattr(node, 'var'):
                signals.extend(self._extract_signals(node.var))
        elif isinstance(node, (ast.IntConst, ast.FloatConst, ast.StringConst)):
            pass  # 常量，不提取信号
        return list(set(signals))  # 去重

    def _get_width(self, node) -> int:
        """获取信号位宽。"""
        if hasattr(node, "width") and node.width:
            if isinstance(node.width, ast.Width):
                msb = self._eval_const(node.width.msb)
                lsb = self._eval_const(node.width.lsb)
                if msb is not None and lsb is not None:
                    return max(msb, lsb) - min(msb, lsb) + 1
        return 1

    def _eval_const(self, node) -> int | None:
        """评估常量表达式。"""
        if node is None:
            return None
        if isinstance(node, ast.IntConst):
            try:
                return int(node.value)
            except ValueError:
                return None
        return None


# ============================================================
# Dataflow 分析：Pyverilog dataflow 模块
# ============================================================

# Pyverilog dataflow 内部临时变量前缀（case 语句展开产物）
_PYVERILOG_TEMP_RE = re.compile(r'^_rn\d+_')


class DataflowAnalyzer:
    """使用 Pyverilog dataflow 模块提取信号依赖。

    结合 AST always 块分析结果，区分 sequential/combinational/control 依赖。
    支持多模块设计：遍历所有子模块的 always 块，建立实例映射。
    """

    def __init__(self, file_list: list[str], top_module: str):
        self.file_list = file_list
        self.top_module = top_module
        self.analyzer: VerilogDataflowAnalyzer | None = None
        self.always_analyzer = AlwaysBlockAnalyzer()
        # 多模块支持
        self.module_analyzers: dict[str, AlwaysBlockAnalyzer] = {}
        self.instance_map: dict[str, str] = {}  # instance_name -> module_type
        self.constants: set[str] = set()  # localparam/parameter 常量名

    def analyze(self) -> dict:
        """执行完整分析，返回 deps_raw.json 格式的字典。"""
        # 1. AST 分析（always 块）
        self._ast_analysis()

        # 2. Dataflow 分析
        self._dataflow_analysis()

        # 3. 组装 deps_raw.json
        return self._assemble_output()

    def _ast_analysis(self):
        """AST 遍历：分析所有模块的 always 块、端口、常量。"""
        ast_tree, directives = pyv_parse(self.file_list)

        if not hasattr(ast_tree, 'description') or not hasattr(ast_tree.description, 'definitions'):
            print("Error: Pyverilog AST has no description.definitions -- file may have failed to parse.", file=sys.stderr)
            sys.exit(1)

        found_modules = []
        top_found = False
        for desc in ast_tree.description.definitions:
            if isinstance(desc, ast.ModuleDef):
                found_modules.append(desc.name)
                analyzer = AlwaysBlockAnalyzer()
                analyzer.visit(desc)
                self.module_analyzers[desc.name] = analyzer
                if desc.name == self.top_module:
                    self.always_analyzer = analyzer
                    top_found = True

                # 收集 localparam/parameter 常量名
                self._collect_constants(desc)

        if not top_found:
            print(f"Error: Top module '{self.top_module}' not found in AST.", file=sys.stderr)
            if found_modules:
                print(f"  Available modules: {found_modules}", file=sys.stderr)
            else:
                print("  No modules found in the provided files.", file=sys.stderr)
            sys.exit(1)

        # 从顶层模块提取实例映射
        self._build_instance_map()

    def _collect_constants(self, module_def):
        """收集模块中的 localparam 和 parameter 声明。"""
        if not hasattr(module_def, 'items') or not module_def.items:
            return
        for item in module_def.items:
            if isinstance(item, ast.Decl):
                for d in item.list:
                    if isinstance(d, (ast.Localparam, ast.Parameter)):
                        if hasattr(d, 'name'):
                            self.constants.add(d.name)

    def _build_instance_map(self):
        """从顶层模块 AST 中提取子模块实例映射。"""
        top_analyzer = self.module_analyzers.get(self.top_module)
        if not top_analyzer:
            return

        # 需要重新解析 AST 来找 Instance 节点
        # 使用 module_analyzers 中已 visit 的顶层模块信息
        # 实际上我们需要从 AST 树中获取，这里通过重新遍历实现
        ast_tree, _ = pyv_parse(self.file_list)
        for desc in ast_tree.description.definitions:
            if isinstance(desc, ast.ModuleDef) and desc.name == self.top_module:
                if desc.items:
                    for item in desc.items:
                        if isinstance(item, ast.InstanceList):
                            module_type = item.module
                            for inst in item.instances:
                                inst_name = inst.name
                                self.instance_map[inst_name] = module_type
                break

    def _dataflow_analysis(self):
        """Pyverilog dataflow 分析：terms + binddict。"""
        self.analyzer = VerilogDataflowAnalyzer(
            self.file_list,
            topmodule=self.top_module,
            noreorder=False,
        )
        self.analyzer.generate()

    def _is_temp_signal(self, name: str) -> bool:
        """判断是否是 Pyverilog dataflow 内部临时变量。"""
        base = name.split(".")[-1] if "." in name else name
        return bool(_PYVERILOG_TEMP_RE.match(base))

    def _is_constant(self, name: str) -> bool:
        """判断是否是 localparam/parameter 常量。"""
        base = name.split(".")[-1] if "." in name else name
        return base in self.constants

    def _resolve_analyzer_for_signal(self, normalized_path: str) -> tuple[AlwaysBlockAnalyzer | None, str]:
        """根据信号路径解析所属模块的 analyzer 和局部信号名。

        normalized_path: 已剥离顶层模块前缀的路径
            - 单模块: "data_o" → (top_analyzer, "data_o")
            - 子模块: "u_ch0.data_o" → (channel_analyzer, "data_o")
        """
        parts = normalized_path.split(".")
        if len(parts) == 1:
            return self.always_analyzer, parts[0]

        # 尝试 instance_name.signal_name
        inst_name = parts[0]
        sig_name = parts[-1]
        module_type = self.instance_map.get(inst_name)
        if module_type and module_type in self.module_analyzers:
            return self.module_analyzers[module_type], sig_name

        return None, normalized_path

    def _iter_sequential_targets(self):
        for signal_name, seq_info in self.always_analyzer.sequential_assigns.items():
            yield f"TOP.{signal_name}", seq_info

        for inst_name, module_type in self.instance_map.items():
            sub_analyzer = self.module_analyzers.get(module_type)
            if not sub_analyzer:
                continue
            for signal_name, seq_info in sub_analyzer.sequential_assigns.items():
                yield f"TOP.{inst_name}.{signal_name}", seq_info

    def _propagate_clock_from_instances(self):
        """从子模块实例传播时钟信息到顶层。

        当子模块有 sequential always 块但顶层没有对应时钟时，
        通过实例端口连接推断时钟传播。
        """
        top_clocks = {c[0] for c in self.always_analyzer.clocks}

        for inst_name, module_type in self.instance_map.items():
            sub_analyzer = self.module_analyzers.get(module_type)
            if not sub_analyzer:
                continue
            for sub_clk, _ in sub_analyzer.clocks:
                if sub_clk and sub_clk not in top_clocks:
                    # 子模块有时钟但顶层没有，尝试从实例端口连接推断
                    # 顶层连接 .clk(clk) → 时钟名就是 clk
                    # 这里简单地将子模块时钟添加到顶层（如果顶层端口中存在同名信号）
                    for port_name, _, _ in self.always_analyzer.ports:
                        if port_name == sub_clk:
                            self.always_analyzer.clocks.append((sub_clk, 0.0))
                            top_clocks.add(sub_clk)
                            break

    def _assemble_output(self) -> dict:
        """组装 deps_raw.json 中间格式。"""
        extract_time = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")

        # 传播子模块时钟到顶层
        self._propagate_clock_from_instances()

        # 1. clocks（去重）
        seen_clocks = set()
        clocks = []
        for clk_name, period in self.always_analyzer.clocks:
            if clk_name and clk_name not in seen_clocks:
                seen_clocks.add(clk_name)
                clocks.append(
                    {
                        "logical_name": clk_name,
                        "waveform_path": f"TOP.{clk_name}",
                        "period_ns": period,
                    }
                )

        # 2. boundary_ports
        boundary_ports = []
        for port_name, direction, width in self.always_analyzer.ports:
            kind = "input_port" if direction in ("IN", "INOUT") else "output_port"
            boundary_ports.append(
                {
                    "path": f"TOP.{port_name}",
                    "direction": direction,
                    "width": width,
                    "kind": kind,
                }
            )

        # 3. edges
        edges = []
        if self.analyzer:
            binddict = self.analyzer.getBinddict()

            for bind_name, binds in binddict.items():
                for bind in binds:
                    dest_str = str(bind.dest)
                    sources = self._tree_to_sources(bind.tree)

                    # 规范化目标信号名
                    dest_parts = dest_str.split(".")
                    if len(dest_parts) > 1:
                        if dest_parts[0] in (self.top_module, "TOP"):
                            dest_normalized = ".".join(dest_parts[1:]) if len(dest_parts) > 2 else dest_parts[-1]
                        else:
                            dest_normalized = dest_str
                    else:
                        dest_normalized = dest_str

                    # 过滤临时变量目标
                    if self._is_temp_signal(dest_normalized):
                        continue

                    # 解析目标信号所属模块
                    target_analyzer, target_local = self._resolve_analyzer_for_signal(dest_normalized)

                    # 过滤后 sources 可能为空（全被 _rn 过滤了）
                    # 对组合逻辑信号，回退到 always 块分析结果
                    filtered_sources = [s for s in sources
                                        if s and s != dest_normalized
                                        and not self._is_temp_signal(s)
                                        and not self._is_constant(s)]

                    # 合并 always 块分析的组合依赖（补充 dataflow 可能遗漏的信号，如 case 比较信号）
                    lookup = target_analyzer or self.always_analyzer
                    if target_local in lookup.combinational_assigns:
                        fallback = [s for s in lookup.combinational_assigns[target_local]
                                    if s and not self._is_constant(s) and s != dest_normalized]
                        existing = set(filtered_sources)
                        for s in fallback:
                            if s not in existing:
                                filtered_sources.append(s)

                    # 检查是否需要自回馈边（FSM 状态寄存器、计数器等）
                    has_self_feedback = False
                    lookup = target_analyzer or self.always_analyzer
                    if target_local in lookup.sequential_assigns:
                        seq_info = lookup.sequential_assigns[target_local]
                        if target_local in seq_info["sources"]:
                            has_self_feedback = True

                    # 为 CE 边去重：同一目标只发一次 CE 边
                    ce_edge_emitted = False

                    for src in filtered_sources:
                        if not src or src == dest_normalized:
                            continue
                        # 过滤临时变量和常量
                        if self._is_temp_signal(src) or self._is_constant(src):
                            continue

                        dep_type = "unknown"
                        inferred_by = "DATAFLOW"
                        clock = None
                        clock_edge = None
                        latency = 0
                        details = ""
                        ce_cond_expr = None

                        # 使用解析到的模块 analyzer 查找类型
                        lookup_analyzer = target_analyzer or self.always_analyzer

                        # 检查是否是 sequential assign
                        if target_local in lookup_analyzer.sequential_assigns:
                            seq_info = lookup_analyzer.sequential_assigns[target_local]
                            # src 可能是局部名或带实例前缀
                            src_local = src.split(".")[-1] if "." in src else src
                            if src_local in seq_info["sources"]:
                                dep_type = "sequential"
                                inferred_by = "FF"
                                clock = seq_info["clock"]
                                clock_edge = seq_info["clock_edge"]
                                latency = 1
                                details = f"FF: {dest_normalized}_reg"

                                # CE 边（每个目标只发一次）
                                # 子模块信号使用实例路径前缀（如 u_ch0.enable → TOP.u_ch0.enable）
                                ce_prefix = f"TOP.{dest_normalized}".rsplit(".", 1)[0] if "." in dest_normalized else "TOP"
                                if not ce_edge_emitted:
                                    if seq_info.get("condition_expression"):
                                        ce_sig = seq_info['ce_signals'][0] if seq_info["ce_signals"] else dest_normalized
                                        ce_source = f"{ce_prefix}.{ce_sig}" if "." in dest_normalized else f"TOP.{ce_sig}"
                                        # 修正 condition_expression 中的路径为实例路径
                                        ce_cond = seq_info["condition_expression"]
                                        if "." in dest_normalized:
                                            inst_prefix = dest_normalized.rsplit(".", 1)[0]
                                            ce_cond = re.sub(
                                                rf'(?<!\.)TOP\.{re.escape(ce_sig)}\b',
                                                f"TOP.{inst_prefix}.{ce_sig}",
                                                ce_cond,
                                            )
                                        ce_edge = {
                                            "source": ce_source,
                                            "target": f"TOP.{dest_normalized}",
                                            "inferred_type": "control",
                                            "inferred_by": "FF_CE",
                                            "condition_expression": ce_cond,
                                            "clock": seq_info["clock"],
                                            "clock_edge": seq_info["clock_edge"],
                                            "latency_cycles": 0,
                                            "details": f"CE pin of FF: {dest_normalized}_reg",
                                        }
                                        edges.append(ce_edge)
                                        ce_edge_emitted = True
                                    elif seq_info["ce_signals"]:
                                        for ce in seq_info["ce_signals"]:
                                            edges.append({
                                                "source": f"{ce_prefix}.{ce}" if "." in dest_normalized else f"TOP.{ce}",
                                                "target": f"TOP.{dest_normalized}",
                                                "inferred_type": "control",
                                                "inferred_by": "FF_CE",
                                                "clock": seq_info["clock"],
                                                "clock_edge": seq_info["clock_edge"],
                                                "latency_cycles": 0,
                                                "details": f"CE pin of FF: {dest_normalized}_reg",
                                            })
                                        ce_edge_emitted = True

                            elif src_local in seq_info["ce_signals"]:
                                dep_type = "control"
                                inferred_by = "FF_CE"
                                clock = seq_info["clock"]
                                clock_edge = seq_info["clock_edge"]
                                latency = 0
                                details = f"CE pin of FF: {dest_normalized}_reg"
                                ce_cond_expr = seq_info.get("condition_expression")

                        # 检查是否是 reset assign
                        if dep_type == "unknown" and target_local in lookup_analyzer.reset_assigns:
                            rst_info = lookup_analyzer.reset_assigns[target_local]
                            src_local = src.split(".")[-1] if "." in src else src
                            if src_local == rst_info[0]:
                                dep_type = "control"
                                inferred_by = "FF_RST"
                                clock = lookup_analyzer.sequential_assigns.get(target_local, {}).get("clock")
                                clock_edge = lookup_analyzer.sequential_assigns.get(target_local, {}).get("clock_edge")
                                latency = 0
                                details = f"RST pin of FF: {dest_normalized}_reg"

                        # 检查是否是 combinational
                        if dep_type == "unknown":
                            if target_local in lookup_analyzer.combinational_assigns:
                                dep_type = "combinational"
                                inferred_by = "DATAFLOW"
                            else:
                                dep_type = "combinational"
                                inferred_by = "NET"

                        edge_dict = {
                            "source": f"TOP.{src}",
                            "target": f"TOP.{dest_normalized}",
                            "inferred_type": dep_type,
                            "inferred_by": inferred_by,
                            "clock": clock,
                            "clock_edge": clock_edge,
                            "latency_cycles": latency,
                            "details": details,
                        }
                        if ce_cond_expr and inferred_by == "FF_CE":
                            edge_dict["condition_expression"] = ce_cond_expr
                        edges.append(edge_dict)

                    # 自回馈边：FSM 状态寄存器、计数器等
                    if has_self_feedback:
                        lookup = target_analyzer or self.always_analyzer
                        seq_info = lookup.sequential_assigns[target_local]
                        edges.append({
                            "source": f"TOP.{dest_normalized}",
                            "target": f"TOP.{dest_normalized}",
                            "inferred_type": "sequential",
                            "inferred_by": "FF",
                            "clock": seq_info["clock"],
                            "clock_edge": seq_info["clock_edge"],
                            "latency_cycles": 1,
                            "details": f"FF: {dest_normalized}_reg (self-feedback)",
                        })

        # 补充缺失的组合信号边（dataflow 无 bind 条目但有 always 块分析）
        # 仅对纯组合信号生效：sequential 信号由 dataflow 处理
        existing_pairs = {(e["source"], e["target"]) for e in edges}
        for target_path, seq_info in self._iter_sequential_targets():
            clock_name = seq_info.get("clock")
            if not clock_name:
                continue
            clock_source = f"TOP.{clock_name}"
            edge_key = (clock_source, target_path)
            if edge_key in existing_pairs:
                continue
            edges.append({
                "source": clock_source,
                "target": target_path,
                "inferred_type": "sequential",
                "inferred_by": "FF",
                "clock": clock_name,
                "clock_edge": seq_info.get("clock_edge"),
                "latency_cycles": 1,
                "details": f"CLK pin of FF: {target_path[4:]}_reg",
            })
            existing_pairs.add(edge_key)

        covered_targets = {e["target"] for e in edges}
        sequential_targets = set(self.always_analyzer.sequential_assigns.keys())
        for sig_name, src_list in self.always_analyzer.combinational_assigns.items():
            if sig_name in sequential_targets:
                continue  # 跳过 sequential 信号，它们的边由 dataflow 处理
            target_path = f"TOP.{sig_name}"
            if target_path in covered_targets:
                continue
            if self._is_temp_signal(sig_name) or self._is_constant(sig_name):
                continue
            valid_sources = [s for s in src_list
                            if s and s != sig_name
                            and not self._is_temp_signal(s)
                            and not self._is_constant(s)
                            and s not in sequential_targets]  # 排除 sequential 信号泄漏
            # 去重
            seen = set()
            for src in valid_sources:
                if src in seen:
                    continue
                seen.add(src)
                edges.append({
                    "source": f"TOP.{src}",
                    "target": target_path,
                    "inferred_type": "combinational",
                    "inferred_by": "DATAFLOW",
                    "clock": None,
                    "clock_edge": None,
                    "latency_cycles": 0,
                    "details": "combinational (always block fallback)",
                })

        # 为 sequential 信号补充 guard 依赖边
        # 例如 count 寄存器的条件依赖 threshold（来自 count < threshold 守卫条件）
        # 这些边类型为 combinational，表示数据路径上的条件依赖
        for sig_name, seq_info in self.always_analyzer.sequential_assigns.items():
            target_path = f"TOP.{sig_name}"
            # 从 sources 中找出非数据源的 guard 信号
            # 数据源 = dataflow bind 中出现的信号（已在上面处理）
            # guard 信号 = sources 中除了直接数据源和自引用之外的信号
            ce_set = set(seq_info.get("ce_signals", []))
            for src in seq_info.get("sources", []):
                if not src or src == sig_name or src in ce_set:
                    continue
                if self._is_temp_signal(src) or self._is_constant(src):
                    continue
                # 检查是否已有边（避免重复）
                edge_key = (f"TOP.{src}", target_path)
                existing = {(e["source"], e["target"]) for e in edges}
                if edge_key in existing:
                    continue
                edges.append({
                    "source": f"TOP.{src}",
                    "target": target_path,
                    "inferred_type": "combinational",
                    "inferred_by": "DATAFLOW",
                    "clock": None,
                    "clock_edge": None,
                    "latency_cycles": 0,
                    "details": f"guard condition for {sig_name} register",
                })

        # 过滤 wire 穿越边：纯端口连线（NET 类型，无功能语义）
        # 特征：source 或 target 是子模块端口且信号名末段相同（如 clk→u_ch0.clk）
        top_ports = {f"TOP.{p[0]}" for p in self.always_analyzer.ports}
        filtered_edges = []
        for e in edges:
            if e["inferred_by"] == "NET" and e["inferred_type"] == "combinational":
                src = e["source"]
                tgt = e["target"]
                src_base = src.split(".")[-1]
                tgt_base = tgt.split(".")[-1]
                # 同名信号直连 = wire passthrough (如 TOP.clk → TOP.u_ch0.clk)
                if src_base == tgt_base:
                    continue
                # 顶层端口 → 子模块端口 或 子模块端口 → 顶层端口（纯连线映射）
                src_is_top_port = src in top_ports
                tgt_is_top_port = tgt in top_ports
                src_is_sub = "." in src[4:]  # TOP. 之后还有 .
                tgt_is_sub = "." in tgt[4:]
                if (src_is_top_port and tgt_is_sub) or (src_is_sub and tgt_is_top_port):
                    continue
            filtered_edges.append(e)
        edges = filtered_edges

        # 去重：移除重复边
        seen_edges = set()
        unique_edges = []
        for e in edges:
            key = (e["source"], e["target"], e["inferred_type"])
            if key not in seen_edges:
                seen_edges.add(key)
                unique_edges.append(e)
        edges = unique_edges

        return {
            "format_version": "1.0",
            "extractor": "pyverilog",
            "extract_time": extract_time,
            "top_module": self.top_module,
            "depth": 2,
            "clocks": clocks,
            "boundary_ports": boundary_ports,
            "modules": [],
            "edges": edges,
        }

    def _tree_to_sources(self, tree) -> list[str]:
        """从 Pyverilog dataflow tree 提取源信号名。

        数据流树节点属性（与 vparser AST 不同）：
        - DFBranch: condnode, truenode, falsenode
        - DFTerminal: name
        - DFOperator: operator, args
        - DFIntConst: value
        """
        if tree is None:
            return []
        sources = []

        def _walk(node):
            if node is None:
                return
            # DFBranch 属性
            if hasattr(node, "condnode"):
                _walk(node.condnode)
            if hasattr(node, "truenode"):
                _walk(node.truenode)
            if hasattr(node, "falsenode"):
                _walk(node.falsenode)
            # 通用属性
            if hasattr(node, "next"):
                _walk(node.next)
            if hasattr(node, "left"):
                _walk(node.left)
            if hasattr(node, "right"):
                _walk(node.right)
            if hasattr(node, "var"):
                _walk(node.var)
            if hasattr(node, "args"):
                for arg in node.args:
                    _walk(arg)
            # 提取终端信号
            if hasattr(node, "name") and node.name is not None:
                name_str = str(node.name)
                # 过滤 Pyverilog 内部临时变量
                if _PYVERILOG_TEMP_RE.search(name_str):
                    return
                parts = name_str.split(".")
                if len(parts) >= 2:
                    # 仅剥离 top module 前缀，保留子模块层次
                    if parts[0] == self.top_module:
                        sources.append(".".join(parts[1:]) if len(parts) > 2 else parts[-1])
                    else:
                        sources.append(name_str)
                else:
                    sources.append(name_str)

        _walk(tree)
        return list(set(sources))


# ============================================================
# 主入口
# ============================================================

def ensure_utf8_encoding(file_list):
    """确保所有 Verilog 文件以 UTF-8 编码读取。

    在 Windows 上，Pyverilog 内部使用系统默认编码（GBK）读取文件，
    如果源文件包含 UTF-8 字符（如中文注释），会导致 UnicodeDecodeError。

    此函数创建临时 UTF-8 编码副本，供 Pyverilog 安全解析。
    """
    temp_dir = None
    utf8_files = []

    try:
        temp_dir = tempfile.mkdtemp(prefix="pyverilog_utf8_")

        for src_file in file_list:
            src_path = Path(src_file)
            if not src_path.exists():
                print(f"Warning: File not found: {src_file}", file=sys.stderr)
                continue

            # 尝试多种编码读取
            content = None
            for encoding in ['utf-8', 'gbk', 'gb2312', 'latin-1']:
                try:
                    with open(src_path, 'r', encoding=encoding, errors='ignore') as f:
                        content = f.read()
                    break
                except (UnicodeDecodeError, LookupError):
                    continue

            if content is None:
                print(f"Error: Cannot decode file: {src_file}", file=sys.stderr)
                continue

            # SystemVerilog → Verilog-2001 降级（对纯 V2001 文件无影响）
            content = normalize_sv_to_v2001(content)

            # 写入 UTF-8 临时文件
            temp_file = Path(temp_dir) / src_path.name
            with open(temp_file, 'w', encoding='utf-8') as f:
                f.write(content)

            utf8_files.append(str(temp_file))

        return utf8_files, temp_dir

    except Exception as e:
        print(f"Error in ensure_utf8_encoding: {e}", file=sys.stderr)
        # 发生错误时返回原始文件列表
        return file_list, None


def main():
    parser = argparse.ArgumentParser(
        description="Extract dependency graph from Verilog source using Pyverilog dataflow analysis"
    )
    parser.add_argument(
        "files", nargs="+", help="Verilog source files (.v)"
    )
    parser.add_argument(
        "-t", "--top", required=True, help="Top module name"
    )
    parser.add_argument(
        "-o", "--output", default="deps_raw.json", help="Output JSON file path (default: deps_raw.json)"
    )

    args = parser.parse_args()

    print(f"Analyzing {len(args.files)} Verilog file(s), top module: {args.top}")

    # 修复 Windows GBK 编码问题：转换为 UTF-8 临时文件
    utf8_files, temp_dir = ensure_utf8_encoding(args.files)

    try:
        analyzer = DataflowAnalyzer(utf8_files, args.top)
        result = analyzer.analyze()

        with open(args.output, "w", encoding="utf-8") as f:
            json.dump(result, f, indent=2, ensure_ascii=False)

        print(f"Written: {args.output}")
        print(f"  Clocks: {len(result['clocks'])}")
        print(f"  Boundary ports: {len(result['boundary_ports'])}")
        print(f"  Edges: {len(result['edges'])}")
    finally:
        # 清理临时目录
        if temp_dir and os.path.exists(temp_dir):
            try:
                shutil.rmtree(temp_dir)
            except Exception as e:
                print(f"Warning: Failed to clean up temp dir {temp_dir}: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
