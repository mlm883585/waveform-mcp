# RTL 依赖图自动提取方案设计

## 1. 目标

本文档定义从 RTL 设计源码自动提取信号依赖图、输出符合 `DEPS_FORMAT.md` 规范的 `deps.yaml` 的技术方案，服务于以下目标：

1. 降低 `deps.yaml` 的维护成本——从纯手工编写变为"自动提取 + 人工审查修正"。
2. 提高依赖图的完整性——自动覆盖所有模块端口连接和寄存器级依赖，减少人工遗漏。
3. 保持与 BFS 根因分析引擎的契约——输出严格遵循 `DEPS_FORMAT.md` 定义的字段语义和时间模型。

---

## 2. 背景与动机

### 2.1 手工编写 deps.yaml 的痛点

| 痛点 | 影响 |
|---|---|
| 大型设计信号众多，人工梳理上下游依赖耗时巨大 | 十万行级代码库难以全量建图 |
| 容易遗漏隐藏的依赖关系 | BFS 追溯链路断裂，无法收敛到根因 |
| 寄存器延迟、BRAM 读延迟等时序信息需要逐条核对 | `latency_cycles` 填写错误导致时间回退偏移 |
| generate 展开后的路径别名需要人工对齐 | `signal_aliases` 维护成本高 |
| 设计变更后依赖图容易过期 | 图与实际 RTL 不一致，BFS 结论不可靠 |

### 2.2 自动提取的定位

自动提取不替代人工判断，而是：

1. 生成 `deps.yaml` 草稿，覆盖模块边界和关键寄存器。
2. 标注需要人工确认的区域（如 CDC 边界、黑盒模块、复杂组合逻辑）。
3. 在设计变更后提供增量更新建议。

---

## 3. 现有方案调研

### 3.1 开源工具对比

| 工具 | 依赖提取能力 | 工作层级 | 许可证 | 语言 | 适配度 |
|---|---|---|---|---|---|
| **Pyverilog** | `dataflow` 模块直接提取信号 source/dest 依赖（Terms + Binds），支持 Graphviz 可视化 | 源码级 | Apache 2.0 | Python | ⭐⭐⭐⭐ |
| **slang + pyslang** | 完整 IEEE 1800-2023 elaboration + 内置 data-flow 分析引擎 + JSON AST 导出 | 源码 + elaborated | MIT | C++/Python | ⭐⭐⭐⭐⭐ |
| **Yosys** | `write_json` 输出完整网表；`dataflow tracking` 命令追踪信号；支持 Tcl/Python 脚本 | 源码→综合后 | ISC | C++/Tcl | ⭐⭐⭐⭐ |
| **Verilator** | `--json-only` 输出 elaborated 设计 JSON；`--dump-dfg` 输出数据流图 | 源码（elaborated） | LGPL | C++ | ⭐⭐⭐ |
| **SpyDrNet** | Python 网表分析框架，基于 NetworkX 图分析，支持 EDIF 和结构 Verilog | 网表级 | BSD | Python | ⭐⭐⭐ |
| **netlist-paths** | 从 Yosys JSON netlist 提取两点间信号路径 | 网表级 | BSD | C++ | ⭐⭐⭐ |
| **Surelog** | CHIPS Alliance 项目，输出 UHDM 标准化中间格式 | 源码 + elaborated | Apache 2.0 | C++ | ⭐⭐ |
| **Verible** | Google 出品，提供语法树和符号表，不直接提供依赖分析 | 源码级 | Apache 2.0 | C++ | ⭐⭐ |
| **tree-sitter-verilog** | 仅提供语法树，不含语义分析或 elaboration | 源码级（纯语法） | MIT | C | ⭐ |

#### 3.1.1 Pyverilog dataflow 模块详解

Pyverilog 是最接近本需求的纯 Python 方案：

```python
from pyverilog.dataflow.dataflow_analyzer import VerilogDataflowAnalyzer

# 提取结果包含：
# - terms: 信号定义（wire/reg/端口声明）
# - binddict: 信号赋值依赖树（谁驱动谁）
#   每个 bind 包含 dest（目标信号）和 tree（源信号的表达式树）
```

**优势**：Python 原生，无需综合，API 直接可用。  
**局限**：不支持 SystemVerilog 高级特性（interface、package、parameterized class）；对 generate 块展开能力有限；不支持 complex 算术推导。

#### 3.1.2 slang 生态详解

slang 是目前 SystemVerilog 支持最完善的开源前端：

- **完整 SV 2023 语法支持**：interface、package、parameterized modules、generate 等。
- **内置 data-flow 分析引擎**：可在 elaborated 设计上追踪信号传播。
- **下游工具链**：
  - `slang-netlist`：专用网表分析工具，从 elaborated 设计提取连接关系。
  - `yosys-slang`：将 slang 作为 Yosys 前端，替代 Yosys 自带的 Verilog parser。
  - `pyslang`：Python 绑定，支持 AST 遍历、elaboration、表达式求值。

**优势**：SV 支持最完整，elaboration 准确。  
**局限**：依赖分析 API 需要在 AST 之上自行构建；内网部署需要编译 C++ 项目。

#### 3.1.3 Yosys 网表方案详解

Yosys 作为开源综合框架，可以通过以下链路提取依赖：

```text
read_verilog → hierarchy → synth → write_json netlist.json
```

`write_json` 输出的 JSON 包含完整的模块、cell、net 和 port 连接信息，可直接解析为依赖图。

**优势**：综合后网表是最真实的连接关系；JSON 输出结构化程度高。  
**局限**：对 Verilog-2005 支持完善，但 SV 支持有限（需 yosys-slang 插件）；综合过程可能优化掉部分信号。

### 3.2 商业 EDA 工具

| 工具 | 厂商 | 信号依赖分析能力 | 评价 |
|---|---|---|---|
| **Verdi** | Synopsys | 信号 source/dest 追踪、nTrace 原理图可视化、自动依赖关系图 | 调试阶段最强信号追踪，但无可编程批量提取 API |
| **SpyGlass** | Synopsys | RTL 静态分析、CDC 分析、信号依赖连接分析、结构/电气规则检查 | 工业标准 RTL lint/CDC 工具，依赖分析能力强，价格昂贵 |
| **Questa + Visualizer** | Siemens EDA | 信号追踪、Schematic viewer、与 ModelSim 深度集成 | 与本项目已有的 ModelSim 20.1.1 同系列 |
| **Xcelium / IMC** | Cadence | 信号连接分析、代码覆盖率追踪 | Cadence 生态，不适用于当前 Vivado + ModelSim 工具链 |

### 3.3 评估结论

| 维度 | 最适合方案 |
|---|---|
| 准确性最高 | Vivado Tcl（elaborated design，处理 generate/参数化最准确） |
| 纯源码级快速分析 | Pyverilog dataflow 模块 |
| SV 支持最完整 | slang + pyslang |
| 综合后网表分析 | Yosys write_json |
| 商业调试追踪 | Verdi nTrace |

---

## 4. 推荐技术路线

### 4.1 双引擎架构

| 引擎 | 定位 | 适用场景 | 优先级 |
|---|---|---|---|
| **Vivado Tcl** | 主引擎 | 需要准确网表级依赖、处理 generate/参数化、大型设计 | P0 |
| **Pyverilog** | 轻量备选 | 快速预览依赖关系、无需打开 Vivado 的场景、CI 集成 | P1 |
| **slang/pyslang** | 远期升级 | 需要完整 SV 支持、替代 Vivado 主引擎 | P2 |

### 4.2 选择理由

| 决策 | 理由 |
|---|---|
| Vivado 为主引擎 | 内网环境已有 Vivado 2018.3；elaborated design 包含完整网表连接，比纯源码解析准确；可正确处理 generate 展开和参数化模块 |
| Pyverilog 为备选 | Python 原生，无需综合，适合快速迭代；可直接集成到 CI/CD；对 Verilog-2001 设计覆盖度足够 |
| 不选 Yosys 为主引擎 | 内网需额外部署；SV 支持有限；综合过程可能优化掉调试所需信号 |
| 不选 Verilator | 输出格式偏调试用途，不是为下游工具消费设计的 |

### 4.3 数据流总览

```text
                     ┌─────────────────────────┐
                     │      RTL 设计源码        │
                     └───────┬─────────────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
              ▼              ▼              ▼
       ┌──────────┐  ┌────────────┐  ┌───────────┐
       │ Vivado   │  │ Pyverilog  │  │ slang     │
       │ Tcl 脚本 │  │ dataflow   │  │ (远期)    │
       └────┬─────┘  └─────┬──────┘  └─────┬─────┘
            │              │               │
            ▼              ▼               ▼
       ┌─────────────────────────────────────────┐
       │         deps_raw.json（中间格式）        │
       └──────────────────┬──────────────────────┘
                          │
                          ▼
               ┌─────────────────────┐
               │  Python 后处理脚本   │
               │  deps_converter.py  │
               └──────────┬──────────┘
                          │
              ┌───────────┼───────────┐
              │           │           │
              ▼           ▼           ▼
        ┌──────────┐  ┌────────┐  ┌──────────────┐
        │deps.yaml │  │ 人工   │  │ annotations  │
        │（草稿）  │  │ 审查   │  │ .yaml（可选）│
        └──────────┘  └────────┘  └──────────────┘
```

---

## 5. 文件结构

```text
wave-analyzer-mcp/
└── tools/
    └── deps-extractor/
        ├── extract_deps.tcl          # Vivado Tcl 主脚本
        ├── extract_deps_batch.tcl    # Vivado 批处理入口（vivado -mode batch -source）
        ├── deps_converter.py         # Python 后处理：deps_raw.json → deps.yaml
        ├── annotations_template.yaml # 手工标注模板（CDC、黑盒等）
        ├── README.md                 # 使用说明
        └── examples/
            ├── sample_deps_raw.json  # 中间格式示例
            └── sample_output.yaml    # 最终 deps.yaml 示例
```

---

## 6. Vivado Tcl 提取方案详细设计

### 6.1 执行方式

支持两种执行方式：

**方式一：Vivado GUI Tcl Console**
```tcl
source extract_deps.tcl
extract_deps -top <top_module> -output deps_raw.json -depth 2
```

**方式二：Vivado 批处理**
```batch
vivado -mode batch -source extract_deps_batch.tcl -tclargs -project project.xpr -top top_module -output deps_raw.json
```

### 6.2 参数定义

| 参数 | 必填 | 默认值 | 说明 |
|---|---|---|---|
| `-project` | 否 | 当前已打开工程 | Vivado 工程文件路径（`.xpr`） |
| `-top` | 是 | — | 顶层模块名 |
| `-output` | 否 | `deps_raw.json` | 输出 JSON 文件路径 |
| `-depth` | 否 | `2` | 层级展开深度（1=仅顶层端口，2=顶层+子模块，3=再深入一层） |
| `-include_internals` | 否 | `true` | 是否提取模块内部寄存器/BRAM 依赖 |
| `-filter_modules` | 否 | 空 | 只提取指定模块的依赖（逗号分隔） |

### 6.3 提取步骤

#### 6.3.1 模块层级提取

```tcl
# 获取所有模块实例（非原语）
set cells [get_cells -hierarchical -filter {IS_PRIMITIVE == "FALSE" && PRIMITIVE_LEVEL != "MACRO"}]

# 对每个模块实例，提取端口列表
foreach cell $cells {
    set pins [get_pins -of_objects $cell -filter {IS_HIERARCHICAL == "FALSE"}]
    foreach pin $pins {
        set direction [get_property DIRECTION $pin]    ;# IN/OUT/INOUT
        set pin_name [get_property NAME $pin]
        set full_path [get_property FULL_NAME $pin]
        # 记录到 nodes 列表
    }
}
```

#### 6.3.2 网表连接提取

```tcl
# 获取指定层级范围内的所有 net
set nets [get_nets -hierarchical -filter {TYPE != "POWER" && TYPE != "GROUND"}]

foreach net $nets {
    set net_name [get_property NAME $net]
    set driver_pins [get_pins -of_objects $net -filter {DIRECTION == "OUT"}]
    set load_pins [get_pins -of_objects $net -filter {DIRECTION == "IN"}]

    # driver → loads 构成依赖关系
    foreach driver $driver_pins {
        foreach load $load_pins {
            # 记录 edge: driver 所属信号 → load 所属信号
        }
    }
}
```

#### 6.3.3 寄存器/时序单元识别

```tcl
# 获取所有触发器
set ffs [get_cells -hierarchical -filter {REF_NAME =~ FD* || REF_NAME =~ LD*}]

foreach ff $ffs {
    set d_pin [get_pins -of_objects $ff -filter {NAME =~ "*D"}]
    set q_pin [get_pins -of_objects $ff -filter {NAME =~ "*Q"}]
    set clk_pin [get_pins -of_objects $ff -filter {NAME =~ "*C" || NAME =~ "*CLK"}]
    set ce_pin [get_pins -of_objects $ff -filter {NAME =~ "*CE" || NAME =~ "*EN"}]
    set rst_pin [get_pins -of_objects $ff -filter {NAME =~ "*R" || NAME =~ "*S"}]

    # D → Q: sequential, latency_cycles=1
    # CE → Q: control
    # 关联 clock: 从 clk_pin 追溯所属 net
}
```

#### 6.3.4 BRAM 单元识别

```tcl
# 获取所有 Block RAM
set brams [get_cells -hierarchical -filter {REF_NAME =~ RAMB*}]

foreach bram $brams {
    # 提取地址端口 → 数据输出端口: memory, latency_cycles=2（默认）
    # 提取使能端口 → 数据输出端口: control
    # 提取写数据端口 → 读数据端口: memory
}
```

#### 6.3.5 时钟网络提取

```tcl
# 获取所有时钟网络
set clk_nets [get_nets -hierarchical -filter {TYPE == "CLOCK"}]

foreach clk_net $clk_nets {
    set clk_name [get_property NAME $clk_net]
    set full_path [get_property FULL_NAME $clk_net]
    # 记录 clock_alias: 逻辑名 → 波形路径
}
```

#### 6.3.6 顶层端口识别

```tcl
# 获取顶层模块的端口
set top_ports [get_ports]

foreach port $top_ports {
    set direction [get_property DIRECTION $port]
    if {$direction == "IN"} {
        # 标记为 boundary, boundary_kind: input_port
    }
    # OUT 端口记录为可能的 BFS 入口
}
```

### 6.4 中间格式：deps_raw.json

```json
{
  "format_version": "1.0",
  "extractor": "vivado_tcl",
  "extract_time": "2026-05-26T10:00:00",
  "top_module": "beam_ctrl",
  "depth": 2,

  "clocks": [
    {
      "logical_name": "clk_sys",
      "waveform_path": "TOP.clk_sys",
      "period_ns": 10.0
    }
  ],

  "boundary_ports": [
    {
      "path": "TOP.cfg_valid",
      "direction": "IN",
      "width": 1,
      "kind": "input_port"
    }
  ],

  "modules": [
    {
      "instance": "TOP.gen_ch__0",
      "canonical": "TOP.ch0",
      "module_type": "beam_channel",
      "ports": [
        {"name": "data_i", "direction": "IN", "width": 16},
        {"name": "data_o", "direction": "OUT", "width": 16}
      ]
    }
  ],

  "edges": [
    {
      "source": "TOP.ch0.data_pipe2",
      "target": "TOP.ch0.data_pipe3",
      "inferred_type": "sequential",
      "inferred_by": "FF",
      "clock": "clk_sys",
      "clock_edge": "posedge",
      "latency_cycles": 1,
      "details": "FDRE: TOP.gen_ch__0.pipe_reg[2]"
    },
    {
      "source": "TOP.ch0.coeff_valid",
      "target": "TOP.ch0.data_pipe3",
      "inferred_type": "control",
      "inferred_by": "FF_CE",
      "clock": "clk_sys",
      "clock_edge": "posedge",
      "latency_cycles": 0,
      "details": "CE pin of FDRE: TOP.gen_ch__0.pipe_reg[2]"
    },
    {
      "source": "TOP.coeff_rd_addr",
      "target": "TOP.coeff_rd_data",
      "inferred_type": "memory",
      "inferred_by": "BRAM",
      "clock": "clk_sys",
      "clock_edge": "posedge",
      "latency_cycles": 2,
      "details": "RAMB36E1: TOP.coeff_bram"
    }
  ]
}
```

---

## 7. Python 后处理方案详细设计

### 7.1 脚本接口

```bash
python deps_converter.py deps_raw.json -o deps.yaml [--annotate annotations.yaml] [--format json|yaml]
```

| 参数 | 说明 |
|---|---|
| `deps_raw.json` | Tcl 提取的中间格式（必选） |
| `-o deps.yaml` | 输出文件路径（默认 `deps.yaml`） |
| `--annotate annotations.yaml` | 手工标注文件，补充 CDC/黑盒/特殊 latency |
| `--format json\|yaml` | 输出格式（默认 yaml） |

### 7.2 依赖类型分类映射

| Tcl 推断来源 | deps.yaml type | deps.yaml category | latency_cycles | 说明 |
|---|---|---|---|---|
| FF D→Q（`inferred_by: "FF"`） | `sequential` | 从上下文推断 | `1` | 单级寄存器 |
| FF CE/EN pin（`inferred_by: "FF_CE"`） | `control` | `control` | `0` | 使能控制 |
| FF RST pin（`inferred_by: "FF_RST"`） | `control` | `control` | `0` | 复位控制 |
| BRAM ADDR→DOUT（`inferred_by: "BRAM"`） | `memory` | `memory` | `2` | BRAM 读延迟（默认 2 周期） |
| BRAM EN pin（`inferred_by: "BRAM_EN"`） | `control` | `control` | `0` | BRAM 使能 |
| 纯 wire 连接（`inferred_by: "NET"`） | `combinational` | 从上下文推断 | `0` | 组合路径 |
| 顶层 IN 端口 | `boundary` | — | `0` | `boundary_kind: input_port` |
| 常量/参数（`inferred_by: "CONST"`） | `boundary` | — | `0` | `boundary_kind: constant` |

### 7.3 category 推断规则

| 信号特征 | category |
|---|---|
| 信号名包含 `valid`、`ready`、`enable`、`en`、`sel`、`state` | `control` 或 `state` |
| 信号名包含 `data`、`din`、`dout`、`addr` | `data` |
| 信号属于 BRAM 端口 | `memory` |
| 信号名包含 `valid && ready` 握手对 | `protocol` |
| 顶层端口 | 从 `input_port` / `output_port` 推断 |
| 其他 | `data`（默认） |

### 7.4 canonical 命名生成

Vivado elaborate 后的路径可能与波形路径不一致，需要生成 `signal_aliases`：

| Vivado 路径模式 | canonical 名 | 说明 |
|---|---|---|
| `TOP.gen_ch__0.data_o` | `TOP.ch0.data_o` | generate 索引 → 逻辑通道号 |
| `TOP.u_bram_ctrl.ram_inst` | `TOP.bram_ctrl.ram` | 实例前缀 `u_` → 逻辑名 |
| `TOP.clk_wiz/clk_out1` | `TOP.clk_sys` | 时钟向导输出 → 逻辑时钟名 |

转换规则可由 `annotations.yaml` 覆盖。

### 7.5 annotations.yaml 扩展点

```yaml
# annotations.yaml —— 手工标注，补充自动提取无法覆盖的语义

# 信号路径覆盖
signal_overrides:
  - vivado_pattern: "TOP.gen_ch__(\\d+)"
    canonical_template: "TOP.ch{0}"

# CDC 边界标注
cdc_boundaries:
  - from_clock: "clk_sys"
    to_clock: "clk_rf"
    signals:
      - "TOP.sys_domain.sync_flag"

# 黑盒模块标注（不展开内部）
blackbox_modules:
  - instance: "TOP.u_ddr_ctrl"
    boundary_kind: blackbox
    description: "DDR 控制器 IP，内部不展开"

# 特殊延迟覆盖
latency_overrides:
  - signal: "TOP.bram_rd_data"
    latency_cycles: 3     # 实际 BRAM 配置为 3 周期读延迟
    clock: "clk_sys"

# 常量/配置信号
constants:
  - signal: "TOP.CFG_MODE"
    value: "2'b01"
    description: "工作模式配置，固定为模式 1"
```

### 7.6 后处理流水线

```text
deps_raw.json
  │
  ├─ 1. 解析 JSON，构建内部图结构
  │
  ├─ 2. 遍历 edges，按 inferred_type 映射为 deps.yaml 依赖类型
  │
  ├─ 3. 合并 annotations.yaml（如有）
  │     ├─ 应用 signal_overrides（正则替换路径）
  │     ├─ 插入 CDC boundary 节点
  │     ├─ 插入 blackbox boundary 节点
  │     └─ 应用 latency_overrides
  │
  ├─ 4. 为无上游的顶层输入端口自动添加 boundary 节点
  │
  ├─ 5. 生成 clock_aliases（从 clocks 列表）
  │
  ├─ 6. 生成 signal_aliases（从 generate 展开路径差异）
  │
  ├─ 7. 推断每个 output 的 category
  │
  └─ 8. 输出 deps.yaml（按 DEPS_FORMAT.md 格式）
```

---

## 8. Pyverilog 备选方案

### 8.1 适用场景

| 场景 | 说明 |
|---|---|
| 快速预览 | 不想打开 Vivado，快速查看信号依赖关系 |
| CI/CD 集成 | 在持续集成中自动检查依赖图变更 |
| 小型设计 | Verilog-2001 设计，无 generate/复杂参数化 |
| 增量分析 | 只分析修改过的模块，快速反馈 |

### 8.2 使用方式

```python
#!/usr/bin/env python3
"""Pyverilog dataflow → deps_raw.json 转换"""

from pyverilog.dataflow.dataflow_analyzer import VerilogDataflowAnalyzer
from pyverilog.dataflow.dataflow_restorer import VerilogDataflowRestorer
import json

def extract_deps_from_verilog(file_list, top_module):
    analyzer = VerilogDataflowAnalyzer(file_list, top_module)
    analyzer.generate()

    terms = analyzer.getTerms()       # 信号定义
    binddict = analyzer.getBinddict() # 信号赋值依赖

    nodes = []
    edges = []

    for term_name, term in terms.items():
        nodes.append({
            "path": f"TOP.{term_name}",
            "type": str(term.termtype),  # Input/Output/Wire/Reg
            "width": term.width
        })

    for bind_name, binds in binddict.items():
        for bind in binds:
            dest = str(bind.dest)
            # 从 bind.tree 提取源信号
            sources = extract_source_signals(bind.tree)
            for src in sources:
                edges.append({
                    "source": f"TOP.{src}",
                    "target": f"TOP.{dest}",
                    "inferred_type": "unknown",  # Pyverilog 不直接区分时序/组合
                    "inferred_by": "DATAFLOW"
                })

    return {"nodes": nodes, "edges": edges}
```

### 8.3 局限与补充

| 局限 | 缓解方式 |
|---|---|
| 不区分时序/组合依赖 | 结合 `always @(posedge clk)` 块分析，将 `always` 块内的赋值标为 `sequential` |
| 不支持 generate 展开 | 在 annotations.yaml 中手工补充 generate 通道 |
| 不支持 SV interface/package | 对这些设计使用 Vivado 主引擎 |
| 无 BRAM/时钟推断 | 由后处理脚本按信号名规则推断 |

### 8.4 Pyverilog 输出到 deps_raw.json

Pyverilog 提取结果复用与 Vivado Tcl 相同的 `deps_raw.json` 格式，只是 `extractor` 字段标记为 `"pyverilog"`，`inferred_type` 初始为 `"unknown"`，由后处理脚本进一步推断。

---

## 9. 远期：slang/pyslang 方案

### 9.1 动机

当以下条件满足时，考虑引入 slang 替代或补充 Vivado 主引擎：

1. 设计大量使用 SystemVerilog 特性（interface、package、parameterized modules）。
2. 需要不依赖 Vivado 的独立 RTL 分析能力。
3. 需要更准确的 elaboration 和类型检查。

### 9.2 集成方式

```python
import pyslang

# 1. 创建 compilation，加载源文件
comp = pyslang.Compilation()
comp.addFile("rtl/top.sv")
comp.addFile("rtl/sub_module.sv")

# 2. Elaborate
root = comp.getRoot()

# 3. 遍历 elaborated 设计
for member in root.members:
    # 提取模块端口、内部信号、连接关系
    ...

# 4. 使用内置 data-flow 分析
# 追踪信号传播路径
```

### 9.3 部署前提

| 前提 | 说明 |
|---|---|
| C++ 编译环境 | slang 需要 CMake + C++17 编译器 |
| Python 绑定 | pyslang 需要 pybind11 |
| 内网分发 | 可预编译为 wheel 或可执行文件分发 |

---

## 10. 与现有系统的集成

### 10.1 与 load_dependencies 的对接

自动提取的 `deps.yaml` 与手工编写的 `deps.yaml` 格式完全一致，可直接通过 `wave-analyzer-mcp` 的 `load_dependencies` 工具加载：

```text
extract_deps.tcl → deps_raw.json → deps_converter.py → deps.yaml
                                                              │
                                                              ▼
                                              load_dependencies("deps.yaml")
                                                              │
                                                              ▼
                                              trace_root_cause / find_fan_in
```

### 10.2 与 WORKFLOW_DESIGN.md 的衔接

在 `WORKFLOW_DESIGN.md` 定义的总体工作流中，本方案位于 Phase 2 阶段：

```text
Phase 1: 仿真脚本固化（sim_config.yaml → transcript + VCD）
Phase 2: ★ 依赖图自动提取（本方案）+ design_spec.yaml 模板化
Phase 3: BFS + 依赖图查询 + AI 调度闭环
```

### 10.3 增量更新流程

设计变更后，无需全量重建依赖图：

1. 重新运行 `extract_deps.tcl`，生成新的 `deps_raw.json`。
2. 运行 `deps_converter.py --diff old_deps.yaml`，输出变更报告。
3. 人工审查变更，决定是否接受更新。

```bash
python deps_converter.py deps_raw.json -o deps_new.yaml --diff deps.yaml
# 输出：
# + Added: TOP.new_module.data_o → TOP.new_module.pipe_reg [sequential]
# - Removed: TOP.old_module.legacy_signal [boundary]
# ~ Changed: TOP.bram_rd_data latency_cycles: 2 → 3
```

---

## 11. 实施计划

### 11.1 阶段划分

| 阶段 | 目标 | 产物 | 依赖 |
|---|---|---|---|
| **M1: Vivado Tcl 原型** | 用 `MINIMAL_REFERENCE_EXAMPLE.md` 的 simple_reg 验证 Tcl 提取 | `extract_deps.tcl` 可运行 | Vivado 2018.3 |
| **M2: Python 后处理** | deps_raw.json → deps.yaml 转换，与手工版对比 | `deps_converter.py` 可运行 | M1 |
| **M3: 真实设计验证** | 在相控阵子模块上运行，验证 generate/BRAM/多通道 | 完整 deps.yaml | M2 |
| **M4: Pyverilog 备选** | 实现 Pyverilog dataflow 提取路径 | `extract_deps_pyverilog.py` | M2 |
| **M5: annotations 扩展** | CDC/黑盒/覆盖标注的完整支持 | `annotations_template.yaml` | M3 |

### 11.2 验收标准

| 阶段 | 验收条件 |
|---|---|
| M1 | simple_reg 的 Tcl 提取结果包含 D→Q sequential 边和 CE control 边 |
| M2 | 自动生成的 deps.yaml 与 `MINIMAL_REFERENCE_EXAMPLE.md` 手工版在边集合上一致 |
| M3 | generate 通道的 signal_aliases 正确解析；BRAM latency_cycles 与 RTL 一致 |
| M4 | Pyverilog 提取的 simple_reg 依赖图与 Vivado 提取结果在边集合上一致 |
| M5 | 标注 CDC boundary 后，BFS 在 CDC 节点正确停止 |

---

## 12. 已知局限与未来方向

### 12.1 当前版本不覆盖的内容

| 局限 | 原因 | 缓解方式 |
|---|---|---|
| CDC 跨时钟域自动识别 | 需要时钟域归属语义，无法仅从网表推断 | annotations.yaml 手工标注 |
| 组合逻辑内部依赖 | 只提取寄存器级和模块端口级，组合中间信号不展开 | 如需深入，后续可集成 Yosys |
| 异步逻辑（Latch/异步复位） | 时序语义不同于同步 FF | 标注为 `control`，latency 需人工确认 |
| 多时钟域信号的自动关联 | 同一信号可能跨域使用 | 依赖 annotations.yaml 标注 |
| 复杂算术推导（如乘法器内部） | 网表层级只有端口连接 | 视为黑盒，标注 boundary |

### 12.2 未来方向

| 方向 | 说明 | 优先级 |
|---|---|---|
| slang/pyslang 集成 | 完整 SV 支持，替代 Vivado 作为主引擎 | P2 |
| 增量依赖图更新 | 只重新提取变更模块，合并到已有图 | P1 |
| 与 digital-assistant 集成 | VSCode 扩展中一键触发依赖图提取 | P2 |
| 依赖图可视化 | 输出 Graphviz DOT 或 HTML 交互式图 | P2 |
| 自动 CDC 检测 | 结合时钟域分析和信号传播路径自动识别 CDC 边界 | P3 |

---

## 13. 与其他文档的关系

| 文档 | 关系 |
|---|---|
| `DEPS_FORMAT.md` | 本方案的输出格式规范；输出必须严格遵循其字段语义 |
| `WORKFLOW_DESIGN.md` | 本方案属于 Phase 2 能力建设 |
| `BFS_ENGINE_DESIGN.md` | 本方案产出是 BFS 的输入依赖图 |
| `SIM_SCRIPTS_DESIGN.md` | Vivado Tcl 提取可在仿真前/后独立执行，不依赖仿真流程 |
| `MINIMAL_REFERENCE_EXAMPLE.md` | 本方案的 M1/M2 阶段验收样例 |
| `DESIGN_SPEC_FORMAT.md` | design_spec.yaml 中的 clock_domains 与本方案的 clock_aliases 需一致 |

如有冲突，以 `DEPS_FORMAT.md` 的字段定义和时间模型为准。
