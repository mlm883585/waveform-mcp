# 最小实现样板

## 1. 目标

本文档提供一个最小可闭环样板，用于指导后续实现与测试以下链路：

```text
design_spec.yaml
    ↓
deps.yaml
    ↓
TB + SVA + transcript.log + dump.vcd
    ↓
parse_assertion_log
    ↓
trace_root_cause
```

样板刻意选择一个 1 级寄存器 + 1 个使能控制信号的简单场景，便于快速验证接口和 BFS。

---

## 2. 设计场景

### 2.1 RTL 语义

被测逻辑抽象如下：

```verilog
always @(posedge clk) begin
  if (!rst_n)
    data_o <= 8'h00;
  else if (enable)
    data_o <= data_i;
end
```

### 2.2 预期行为

| 项目 | 预期 |
|---|---|
| 正常情况 | `enable=1` 时，`data_o` 在下一次检查点应反映上一拍输入 |
| 异常情况 | 若测试场景期望一次传输，但采样拍 `enable=0`，则输出保持旧值，形成断言失败 |

---

## 3. 最小 `design_spec.yaml`

```yaml
spec_version: "1.0"
module_name: "simple_reg"
description: "单级寄存器与使能控制样板"

clock_domains:
  - name: "clk"
    period_ns: 20.0
    edge: posedge

resets:
  - name: "rst_n"
    active_level: low
    synchronous: true
    related_clock: "clk"

requirements:
  - id: "REQ-001"
    title: "使能拉高后数据应传递到输出"
    description: "enable 为 1 时，data_i 经 1 个周期后出现在 data_o"
    priority: critical
    related_signals:
      - "TOP.data_o"
      - "TOP.enable"
    related_clocks:
      - "clk"

behaviors:
  - id: "BEH-001"
    requirement_ids: ["REQ-001"]
    description: "单周期数据传递，最终以断言结果为准"
    kind: latency
    check_engine: sva
    pass_criteria: "最终通过/失败由 `assert_data_transfer` 判定；behavior 主要提供入口信号与时钟语义"
    fail_entry_signals:
      - "TOP.data_o"
      - "TOP.enable"
    related_clock: "clk"

assertions:
  - name: "assert_data_transfer"
    requirement_ids: ["REQ-001"]
    description: "测试场景要求传输时，输出必须在下一拍更新"
    clock: "clk"
    severity: error
    observe_signals:
      - "TOP.data_o"
      - "TOP.data_i"
      - "TOP.enable"
    sva: |
      assert property (@(posedge clk)
        tb_expect_transfer |-> ##1 data_o == $past(data_i))
      else $error("data transfer failed");
```

其中 `tb_expect_transfer` 是 TB 局部“本拍应发生一次传输”的期望标志，不作为 BFS 入口信号。

---

## 4. 最小 `deps.yaml`

```yaml
format_version: "1.0"
description: "simple_reg 样板依赖图"

clock_aliases:
  - clock_name: "clk"
    modelsim: "TOP.clk"

dependencies:
  - output: "TOP.data_o"
    category: data
    depends_on:
      - signal: "TOP.data_i"
        type: sequential
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: "="
      - signal: "TOP.enable"
        type: control
        clock: "clk"
        edge: posedge
        latency_cycles: 1
        check: ">0"

  - output: "TOP.enable"
    category: control
    depends_on:
      - signal: "TOP.enable"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null

  - output: "TOP.data_i"
    category: data
    depends_on:
      - signal: "TOP.data_i"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
```

本样板中 `deps.yaml.clock=clk` 使用逻辑时钟名；运行时通过 `clock_aliases` 把它解析到实际波形时钟 `TOP.clk`。

---

## 5. 最小 transcript 样板

```text
# ** Error: (vsim-10142) TOP.tb_top.assert_data_transfer:
#    Time: 30 ns  Scope: tb_top File: tb/tb_top.sv Line: 42
```

对应 `parse_assertion_log` 的期望输出：

```json
{
  "events": [
    {
      "assertion_name": "assert_data_transfer",
      "severity": "error",
      "scope_path": "tb_top",
      "time_value": 30,
      "time_unit": "ns",
      "time_ps": 30000,
      "source_file": "tb/tb_top.sv",
      "source_line": 42
    }
  ]
}
```

这里默认失败由 TB 中基于 `tb_expect_transfer` 的断言触发；Transcript 只提供失败名称与时间，不承担失败入口信号解析职责。

---

## 6. 最小 VCD 样板

### 6.1 语义说明

这里故意制造失败。TB 在 `10ns` 这个上升沿前声明“本拍应发生一次传输”，但 DUT 内部 `enable` 被错误保持为 `0`，因此到下一次检查点时 `data_o` 仍未更新。

| 时刻 | `clk` | `enable` | `data_i` | `data_o` | 说明 |
|---|---|---|---|---|---|
| 0ns | 0 | 0 | `00` | `00` | 初始值 |
| 10ns | 上升沿 | 0 | `5A` | `00` | TB 期望传输，但使能错误为 0，寄存器未采样 |
| 20ns | 下降沿 | 0 | `5A` | `00` | 中间观察点，无新数据进入输出 |
| 30ns | 上升沿/检查点 | 0 | `5A` | `00` | 断言检查前一拍应到达的输出，失败 |

### 6.2 VCD 内容

```text
$date 2026-05-09 $end
$version minimal example $end
$timescale 1ns $end
$scope module TOP $end
$var wire 1 ! clk $end
$var wire 1 " enable $end
$var wire 8 # data_i $end
$var wire 8 $ data_o $end
$upscope $end
$enddefinitions $end
#0
0!
0"
b00000000 #
b00000000 $
#10
1!
0"
b01011010 #
#20
0!
#30
1!
0"
b00000000 $
```

> 这里故意省略了 TB 局部信号 `tb_expect_transfer` 的 VCD 记录；BFS 只需要 DUT 信号即可复盘根因。

### 6.3 对应 TB 驱动伪代码

下面给出一个与上表时序一致的最小 TB 驱动顺序，目的是避免把 stimulus 写成“与采样边沿同一时刻变化”而造成语义歧义：

```systemverilog
initial begin
  rst_n = 0;
  tb_expect_transfer = 0;
  enable = 0;
  data_i = 8'h00;

  repeat (1) @(negedge clk);
  rst_n = 1;

  @(negedge clk);
  data_i = 8'h5A;
  tb_expect_transfer = 1;   // 声明下一次 posedge 应发生传输
  enable = 0;               // 故意制造错误：采样拍仍为 0

  @(posedge clk);           // 10ns：DUT 未采样到传输
  tb_expect_transfer = 0;

  @(posedge clk);           // 30ns：断言检查上一拍应到达的输出，失败
end
```

---

## 7. 最小 `run_summary.json`

```json
{
  "status": "assertion_failed",
  "compile_ok": true,
  "elab_ok": true,
  "simulation_ok": true,
  "assertion_fail_count": 1,
  "wave_file": "sim_output/dump.vcd",
  "transcript_file": "sim_output/transcript.log",
  "top_module": "tb_top"
}
```

---

## 8. 预期分析流程

| 步骤 | 输入 | 预期结果 |
|---|---|---|
| 1 | `run_summary.json` | 判定 `status=assertion_failed` |
| 2 | `transcript.log` | 解析到 `assert_data_transfer @ 30ns` |
| 3 | `design_spec.yaml` | 从 `observe_signals` 选择主入口：`TOP.data_o` |
| 4 | `deps.yaml` | 加载依赖图与 `clock_aliases`，确认 `TOP.data_o` 扇入 |
| 5 | `dump.vcd` | 从故障点读取 `TOP.data_o=00` |
| 6 | BFS | 回退 1 个 `clk` 上升沿，读取 `TOP.data_i=5A`、`TOP.enable=0` |
| 7 | 候选根因 | `TOP.enable` 应被列为高优先级候选，`TOP.data_i` 作为已通过的上下文节点保留 |

---

## 9. 预期 `trace_root_cause` 输出

### 9.1 文本模式

```text
Root: TOP.data_o @ time_index=3
  TOP.data_o = 8'h00 [Suspect]
  ├─ TOP.data_i @ previous posedge(clk) = 8'h5A [Ok]
  └─ TOP.enable @ previous posedge(clk) = 1'b0 [RootCauseCandidate]
```

### 9.2 JSON 关键字段

```json
{
  "root_signal": "TOP.data_o",
  "candidates": [
    {
      "signal_path": "TOP.enable",
      "status": "RootCauseCandidate",
      "reason": "数据路径输入正确，但控制信号关闭导致输出未更新"
    }
  ]
}
```

---

## 10. 对应测试建议

| 测试文件 | 建议内容 |
|---|---|
| `tests/assertion_tests.rs` | 用样板 transcript 校验断言事件解析 |
| `tests/deps_tests.rs` | 用样板 deps 校验 fan-in 和 boundary |
| `tests/bfs_tests.rs` | 用样板 VCD + deps 校验 BFS 候选根因是 `TOP.enable` |

---

## 11. 扩展方式

在这个最小样板通过后，可按以下顺序扩展：

1. 把单级寄存器扩展为 3 级流水线。
2. 增加 BRAM 读延迟边。
3. 增加 generate 通道 alias。
4. 增加 CDC boundary。
5. 替换为真实相控阵子模块的 spec/deps/TB。

---

## 12. 关联文档

| 文档 | 作用 |
|---|---|
| `INTERFACE_CONTRACTS.md` | 约束工具输入输出格式 |
| `BFS_ENGINE_DESIGN.md` | 定义 BFS 算法语义 |
| `DESIGN_SPEC_FORMAT.md` | 约束 spec 字段 |
| `DEPS_FORMAT.md` | 约束依赖图字段 |

本文档负责提供第一套可直接拿来开发和写测试的参考样板。
