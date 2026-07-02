# 设计需求文档格式规范

## 1. 目标

`design_spec.yaml` 是 AI 驱动验证流程的起点文件，用于：

1. 定义模块需求与接口约束。
2. 定义可验证行为、测试场景和断言入口。
3. 为波形分析和 BFS 提供“从哪个信号开始看”的依据。

本文件不直接替代 `deps.yaml`。规格负责定义“应该发生什么”，依赖图负责定义“失败后往哪里追”。

---

## 2. 适用范围

| 项目 | 约束 |
|---|---|
| RTL | Verilog，兼容 Vivado 2018.3 |
| 仿真 | ModelSim 20.1.1.720 |
| 测试 | TB + SVA |
| AI 使用方式 | 读取 spec 后生成/审查 RTL、TB、断言、分析入口 |

---

## 3. 顶层结构

```yaml
spec_version: "1.0"
module_name: "<模块名>"
description: "<模块描述>"
owners:
  designer: "<设计负责人>"
  verifier: "<验证负责人>"
environment:
  rtl_language: "verilog"
  simulator: "modelsim"
  synthesis_tool: "vivado_2018_3"

clock_domains:
  - name: "<clk>"
    period_ns: 5.0
    edge: posedge
    description: "<说明>"

resets:
  - name: "<rst_n>"
    active_level: low
    synchronous: true
    related_clock: "<clk>"

ports:
  - name: "<端口名>"
    direction: input
    width: 16
    clock_domain: "<clk或null>"
    description: "<说明>"

interfaces:
  - name: "<接口名>"
    type: axi_stream | custom
    role: master | slave
    signals:
      valid: "<信号名>"
      ready: "<信号名>"
      data: "<信号名>"

requirements:
  - id: "REQ-001"
    title: "<需求标题>"
    description: "<需求描述>"
    priority: critical | high | medium | low
    related_signals:
      - "<顶层或 TB 可观察信号>"
    related_clocks:
      - "<clk>"

behaviors:
  - id: "BEH-001"
    requirement_ids: ["REQ-001"]
    description: "<行为说明>"
    kind: invariant | event | latency | protocol
    check_engine: wave_analyzer_mcp | sva
    check: "<条件表达式>"
    pass_criteria: "<文字描述>"
    fail_entry_signals:
      - "<失败后进入 BFS 的第一批信号>"
    related_clock: "<clk>"
    active_window: "<可选，行为生效阶段说明>"

assertions:
  - name: "assert_xxx"
    requirement_ids: ["REQ-001"]
    description: "<断言意图>"
    clock: "<clk>"
    severity: error | warning | note
    observe_signals:
      - "<断言失败后优先追踪的信号>"
    sva: |
      <SystemVerilog Assertion>

test_scenarios:
  - id: "TC-001"
    purpose: "<测试目的>"
    stimulus_summary: "<激励摘要>"
    checks:
      behaviors: ["BEH-001"]
      assertions: ["assert_xxx"]

debug_hints:
  entry_points:
    - signal: "<建议调试入口信号>"
      reason: "<为什么从这里开始>"
  stop_signals:
    - "<BFS 可停止的边界信号>"

coverage_targets:
  - name: "<覆盖项>"
    target: "<目标>"
```

---

## 4. 关键字段说明

### 4.1 `requirements`

需求项是后续所有检查的追踪源。

| 字段 | 作用 |
|---|---|
| `id` | 稳定编号，供 behavior/assertion/test 关联 |
| `related_signals` | 规格层面最关心的输出或状态信号 |
| `related_clocks` | 行为检查所依赖的时钟域 |

### 4.2 `behaviors`

`behaviors` 描述“如何从波形侧判断设计是否符合需求”。

| 字段 | 作用 |
|---|---|
| `kind` | 行为类别，便于 AI 选择验证方式 |
| `check_engine` | `wave_analyzer_mcp` 或 `sva` |
| `check` | 当 `check_engine=wave_analyzer_mcp` 时使用条件表达式 |
| `fail_entry_signals` | 该行为失败后，BFS 优先从哪些信号入手 |
| `related_clock` | 波形检查和时间解释的基准时钟 |

### 4.3 `assertions`

断言用于仿真中实时检查。

注意：

1. Transcript 只保证给出断言名、严重级别、时间、scope 等日志信息。
2. Transcript 不能稳定提供“具体失败信号”。
3. 因此必须在 spec 中显式写出 `observe_signals`，供失败后进入 BFS。

这比让 AI 运行时去解析整段 SVA 再猜信号更可靠。

### 4.4 `debug_hints`

这是面向 AI 与调试工具的辅助字段。

| 字段 | 作用 |
|---|---|
| `entry_points` | 手动/自动调试时优先查看的信号 |
| `stop_signals` | BFS 可以接受的边界，如接口输入、寄存器文件输出、CDC 边界 |

---

## 5. 条件表达式使用规则

### 5.1 当前条件引擎适用边界

| 能力 | 支持情况 |
|---|---|
| 信号路径 | 支持 |
| 逻辑运算 `&&` `||` `!` | 支持 |
| 位运算 `~` `&` `|` `^` | 支持 |
| 比较 `==` `!=` | 支持 |
| 位提取 `[msb:lsb]` | 支持 |
| Verilog 字面量 | 支持 |
| `$past(sig)` | 支持 |
| `$past(sig, N)` | 不支持 |
| 算术运算 `+ - * /` | 不支持 |
| `$isunknown` | 不支持 |

### 5.2 使用建议

| 场景 | 建议 |
|---|---|
| 简单握手、边沿、固定延迟关系 | `wave_analyzer_mcp` |
| 算术正确性、X/Z 检测、复杂时序蕴含 | `sva` |
| 关键行为 | `wave_analyzer_mcp + sva` 双重覆盖 |

强约束：

1. `wave_analyzer_mcp` 的条件检查默认是“事件/观察点搜索工具”，不是完整的时序证明器。
2. 只要需求包含“整个窗口始终成立”“复杂算术关系”“X/Z 严格语义”“跨多拍蕴含”，就不应仅依赖 `wave_analyzer_mcp` 判定通过。
3. 若某行为会作为最终 sign-off 依据，建议至少配一条 SVA 或等价的仿真内硬判定。

### 5.3 注意事项

`find_conditional_events` 更适合找“发生过什么事件”，不适合单独证明“整个仿真窗口始终满足某不变量”。因此：

1. `kind=event`、`kind=latency` 更适合条件搜索。
2. `kind=invariant` 的关键场景建议配套 SVA。

---

## 6. 与 BFS 的衔接规则

### 6.1 失败到 BFS 的标准链路

| 来源 | 如何得到入口信号 |
|---|---|
| `behavior` 失败 | 直接使用 `fail_entry_signals` |
| `assertion` 失败 | 使用断言的 `observe_signals` |
| 人工波形观察发现异常 | 使用 `debug_hints.entry_points` 或人工指定 |

补充约束：

1. `observe_signals` / `fail_entry_signals` 可以是列表，但单次 BFS 调用只接受一个入口信号。
2. 因此 spec 最好把最主要的失败观察点放在列表首位，便于上层先选主入口。
3. 其余入口信号用于二次追溯或补充分支，不建议在第一版接口里一次性并行展开。

### 6.2 不建议的做法

以下设计不建议采用：

1. 断言失败后再让 AI 去自由解析 SVA 文本猜测入口信号。
2. spec 中只写自然语言，不写可追踪的入口信号。
3. 让 `pipeline_specs` 直接替代 `deps.yaml`。

---

## 7. 推荐模板

```yaml
spec_version: "1.0"
module_name: "beam_ctrl"
description: "波束控制与系数装载模块"
owners:
  designer: "fpga_team"
  verifier: "verification_team"
environment:
  rtl_language: "verilog"
  simulator: "modelsim"
  synthesis_tool: "vivado_2018_3"

clock_domains:
  - name: "clk_sys"
    period_ns: 5.0
    edge: posedge
    description: "系统主时钟"

resets:
  - name: "rst_n"
    active_level: low
    synchronous: true
    related_clock: "clk_sys"

ports:
  - name: "cfg_valid"
    direction: input
    width: 1
    clock_domain: "clk_sys"
    description: "配置输入有效"
  - name: "coeff_addr"
    direction: output
    width: 10
    clock_domain: "clk_sys"
    description: "系数读取地址"
  - name: "beam_data_o"
    direction: output
    width: 16
    clock_domain: "clk_sys"
    description: "波束输出数据"

requirements:
  - id: "REQ-001"
    title: "配置后 3 周期输出新系数路径"
    description: "配置握手完成后，3 个 clk_sys 周期内系数应完成装载并参与输出"
    priority: critical
    related_signals:
      - "TOP.beam_data_o"
      - "TOP.coeff_valid"
    related_clocks:
      - "clk_sys"

behaviors:
  - id: "BEH-001"
    requirement_ids: ["REQ-001"]
    description: "配置有效后，系数有效信号应在 3 周期后拉高"
    kind: latency
    check_engine: wave_analyzer_mcp
    check: "$past($past($past(TOP.cfg_valid))) == TOP.coeff_valid"
    pass_criteria: "每次 cfg_valid 生效后，coeff_valid 都在 3 周期后成立"
    fail_entry_signals:
      - "TOP.coeff_valid"
      - "TOP.coeff_addr"
    related_clock: "clk_sys"

assertions:
  - name: "assert_coeff_valid_latency"
    requirement_ids: ["REQ-001"]
    description: "配置到系数有效的时延必须为 3 周期"
    clock: "clk_sys"
    severity: error
    observe_signals:
      - "TOP.coeff_valid"
      - "TOP.coeff_addr"
      - "TOP.cfg_valid"
    sva: |
      assert property (@(posedge clk_sys)
        cfg_valid |-> ##3 coeff_valid)
      else $error("coeff_valid latency mismatch");

test_scenarios:
  - id: "TC-001"
    purpose: "单次配置装载"
    stimulus_summary: "发送一次 cfg_valid 和配置字"
    checks:
      behaviors: ["BEH-001"]
      assertions: ["assert_coeff_valid_latency"]

debug_hints:
  entry_points:
    - signal: "TOP.beam_data_o"
      reason: "用户最先观察到的异常输出"
    - signal: "TOP.coeff_valid"
      reason: "系数链路常见故障入口"
  stop_signals:
    - "TOP.cfg_valid"
    - "TOP.cfg_data"
```

---

## 8. 与其他文档的边界

| 文档 | 边界 |
|---|---|
| `DEPS_FORMAT.md` | 定义上游依赖关系，不在 spec 中重复建图 |
| `BFS_ENGINE_DESIGN.md` | 定义失败后如何追溯，不在 spec 中定义算法 |
| `SIM_SCRIPTS_DESIGN.md` | 定义如何运行仿真，不在 spec 中定义脚本参数 |

如果 spec 字段与其他文档冲突，以本文件的“需求/入口定义”角色为准，但不得越权取代依赖图和仿真配置。
