# BFS 根因追溯算法 — 在波形验证中的原理与应用

## 写给谁看

这份文档面向对 FPGA/ASIC 验证有基本了解、但对 BFS 根因追溯算法不熟悉的同事。
读完本文，你将理解：

- **为什么**需要这个算法（它解决了什么痛点）
- **是什么**（核心概念用大白话解释）
- **怎么用**（完整的操作流程）
- **什么时候用**（适用场景和限制）

---

## 一、痛点：仿真失败了，然后呢？

### 1.1 传统调试流程

你跑了一次仿真，ModelSim 输出了一行报错：

```
# ** Error: (vsim-10143) Assertion error: assert_data_valid failed at tb_top.dut at 1750 ns
```

这条信息告诉了你 **三件事**：

1. **什么** 失败了 —— `assert_data_valid`
2. **在哪里** 失败 —— `tb_top.dut`
3. **什么时候** 失败 —— `1750 ns`

但它 **没有** 告诉你：

- **为什么** 失败了
- 是哪个上游信号导致的
- 这个错误是从哪里传播过来的

### 1.2 人工排查的困境

传统做法是：打开波形文件（VCD/FST），找到失败的信号，然后 **手动** 往回追：

```
data_o 错了 --> 看看 data_i 对不对 --> 看看 enable 信号对不对 --> 看看 upstream 的控制逻辑 ...
```

每一步你都要：

1. 在波形中找到对应信号
2. 找到正确的时间点（要考虑时钟沿和流水线延迟）
3. 读值、比较、判断
4. 决定下一步追哪个信号

对于一个有 10 万+ 行 RTL 代码的设计，这个过程可能重复几十次，每次追 5-10 个信号。
**这就是 BFS 算法要自动化的事情。**

---

## 二、核心思路：把问题分解成两个部分

### 2.1 两个关键文件

BFS 算法依赖两个输入文件：

| 文件 | 内容 | 类比 |
|------|------|------|
| `dump.vcd` / `dump.fst` | 仿真波形，记录了所有信号在每个时间点的值 | **"病历"** —— 实际发生了什么 |
| `deps.yaml` | 信号之间的依赖关系图 | **"解剖图"** —— 信号之间是怎么连接的 |

**波形文件**告诉你"在这个时间点，这个信号的值是什么"。
**依赖图**告诉你"这个信号的值是从哪些上游信号来的"。

BFS 做的就是：**从失败的信号出发，沿着依赖图往回追，同时读波形中的实际值，自动判断每一段是正常还是异常。**

### 2.2 依赖图（deps.yaml）长什么样？

用一个简单的寄存器例子：

```yaml
# deps.yaml — 简化的依赖图
nodes:
  - name: "TOP.data_o"
    category: data
  - name: "TOP.data_i"
    category: data
  - name: "TOP.enable"
    category: control

clock_aliases:
  clk:
    modelsim: "tb_top.dut.clk"

dependencies:
  - output: "TOP.data_o"
    depends_on:
      - signal: "TOP.data_i"
        type: sequential          # 寄存器依赖：data_i 经过一个时钟周期到达 data_o
        clock: "clk"
        edge: posedge
        latency_cycles: 1         # 延迟 1 个时钟周期
        check: "="                # 检查：data_i 的值应该等于 data_o 的值
      - signal: "TOP.enable"
        type: control             # 控制信号：enable 门控数据输出
        clock: "clk"
        edge: posedge
        latency_cycles: 0         # 控制信号组合逻辑，不需要回溯
        check: ">0"               # 检查：enable 应该为真（非零）
```

这里有几个关键概念：

#### 依赖类型（type）

| 类型 | 含义 | 时间处理 | 例子 |
|------|------|----------|------|
| `combinational` | 组合逻辑，同一拍 | 不回溯 | `y = a & b` |
| `sequential` | 寄存器/流水线，需要回溯时钟 | 回溯 N 个时钟沿 | 触发器输出 |
| `memory` | BRAM/寄存器堆 | 回溯 N 个时钟沿 | BRAM 读数据 |
| `control` | 使能/选择/门控信号 | latency=0 不回溯，>0 回溯 | `enable`, `sel` |
| `protocol` | 握手/流控协议 | latency=0 不回溯，>0 回溯 | `valid/ready` |
| `boundary` | 调试边界，停止追溯 | 不回溯 | 输入端口、CDC 边界 |

#### 检查表达式（check）

| 表达式 | 含义 | 通过条件 |
|--------|------|----------|
| `"="` | 值相等 | 上游值 == 下游值 |
| `"!="` | 值不等 | 上游值 != 下游值 |
| `">0"` | 非零 | 信号值 > 0 |
| `"==0"` | 为零 | 信号值 == 0 |

#### 时钟回溯（最关键的概念）

**BFS 不用 "时间索引 - N" 来回溯。**

VCD/FST 文件中的时间索引是事件发生的时间戳，不是时钟周期编号。
一个时钟周期可能跨越多个时间索引，有些周期可能没有任何事件。

**正确做法：**

1. 从波形中提取时钟信号的所有上升沿（或下降沿）
2. 建立一个"时钟边沿表"：`[边沿0的时间索引, 边沿1的时间索引, 边沿2的时间索引, ...]`
3. 从当前时间索引，在边沿表中找到最近的边沿
4. 往前数 N 个边沿，得到上游的观察时间

```
时钟边沿表: [5, 15, 25, 35, 45, 55, ...]
                       ↑ 当前 time_index = 35
回溯 2 个周期: 找到 time_index = 15
```

---

## 三、BFS 算法详解

### 3.1 算法流程图

```
                    ┌─────────────────────┐
                    │  仿真报错            │
                    │  assertion failed    │
                    └─────────┬───────────┘
                              │
                              ▼
                    ┌─────────────────────┐
                    │  解析报错信息        │
                    │  名称/时间/作用域    │
                    └─────────┬───────────┘
                              │
                              ▼
                    ┌─────────────────────┐
                    │  确定入口信号        │
                    │  (Entry Signal)     │
                    └─────────┬───────────┘
                              │
                              ▼
              ┌───────────────────────────────┐
              │     BFS 追溯（核心算法）        │
              │                               │
              │  ┌─────┐                      │
              │  │队列 │ → 弹出节点            │
              │  └─────┘   │                  │
              │     ▲      ▼                  │
              │     │  ┌─────────┐            │
              │     │  │读波形值 │            │
              │     │  └────┬────┘            │
              │     │       ▼                 │
              │     │  ┌─────────┐            │
              │     │  │查fan-in │            │
              │     │  │依赖边   │            │
              │     │  └────┬────┘            │
              │     │       ▼                 │
              │     │  ┌─────────┐            │
              │     │  │时钟回溯 │            │
              │     │  │+读值    │            │
              │     │  └────┬────┘            │
              │     │       ▼                 │
              │     │  ┌─────────┐            │
              │     │  │检查     │──Ok──┐     │
              │     │  │表达式   │      │     │
              │     │  └────┬────┘      │     │
              │     │       │Suspect    │     │
              │     │       ▼           │     │
              │     │  ┌─────────┐      │     │
              │     └──│入队    │      │     │
              │        │Suspect  │◄─────┘     │
              │        └─────────┘            │
              └───────────────┬───────────────┘
                              │
                              ▼
                    ┌─────────────────────┐
                    │  输出追溯树          │
                    │  + 根因候选列表      │
                    └─────────────────────┘
```

### 3.2 节点状态（8 种）

BFS 追溯过程中，每个节点都有一个状态，表示"这段信号路径是否正常"：

| 状态 | 含义 | 通俗解释 | 是否继续追溯 |
|------|------|----------|:---:|
| `Suspect` | 可疑 | "这段可能有问题，继续查上游" | ✅ |
| `RootCauseCandidate` | 根因候选 | "上游都没问题，问题就在这儿！" | ✅ |
| `Ok` | 正常 | "这段没问题，上游和下游一致" | ❌ |
| `Boundary` | 边界 | "到了设计边界（输入端口/CDC/黑盒），没法再追了" | ❌ |
| `Stopped` | 停止 | "用户说停到这里就不追了" | ❌ |
| `Truncated` | 截断 | "追太深了，达到最大深度限制" | ❌ |
| `Cyclic` | 环路 | "遇到了环形依赖（自己依赖自己），跳过" | ❌ |
| `Context` | 上下文 | "没有检查表达式，仅供参考" | ❌ |

### 3.3 状态判定逻辑（核心）

#### 边级别判定（`evaluate_edge_status`）

对于每一条依赖边，比较上游信号和下游信号的值：

```
检查表达式 "="：
  上游值 == 下游值  →  Ok（一致，没问题）
  上游值 != 下游值  →  Suspect（不一致，有问题！）

检查表达式 ">0"：
  信号值 > 0  →  Ok（使能信号有效）
  信号值 = 0  →  Suspect（使能信号无效，可能是问题根源）
```

#### 节点级别判定（`summarize_node_status`）

一个节点可能有多个上游依赖（多个 fan-in 边），需要综合判断：

```
如果所有上游都 Ok，但本节点仍 Suspect
  → 升级为 RootCauseCandidate（根因候选！）

如果有上游是 Suspect
  → 保持 Suspect（问题可能在上游，继续追）

如果有上游是 Boundary
  → 标记为 Boundary（到了边界）

如果所有上游都 Ok，且本节点也是 Ok
  → 标记为 Ok（整段都正常）
```

**RootCauseCandidate 的判定是算法的核心价值：**

> 当一个信号的所有上游输入都正确（check 都通过），但这个信号本身的值却是错的——
> 说明问题 **就在这儿或者更上游但未建模**。这是最高优先级的根因候选。

### 3.4 具体例子：三级流水线

假设有一个三级流水线，数据从 `stage0` → `stage1` → `stage2` → `output`：

```
时钟周期:    0      1      2      3
stage0:    [A] ─→ [A] ─→ [A] ─→ [A]
stage1:    [?] ─→ [A] ─→ [A] ─→ [A]
stage2:    [?] ─→ [?] ─→ [A] ─→ [A]
output:    [?] ─→ [?] ─→ [?] ─→ [B]  ← 期望是 A，实际是 B！
```

`assert_output_correct` 在周期 3 失败。BFS 追溯过程：

**第 1 步：从 output @ 周期3 开始（状态 = Suspect）**

**第 2 步：查 fan-in**
- `output` 依赖 `stage2`（sequential, latency=1）
- `output` 依赖 `enable_out`（control, latency=0）

**第 3 步：读值 + 检查**

| 信号 | 时间 | 值 | 检查 | 结果 |
|------|------|-----|------|------|
| stage2 | 周期2 | B | `=` (output=B, stage2=B) | Ok |
| enable_out | 周期3 | 1 | `>0` | Ok |

**第 4 步：判定**
- 所有上游都 Ok，但 output 本身是 Suspect
- → **output 升级为 RootCauseCandidate！**

但等等，stage2 的值也是 B，为什么它是 Ok？因为 check 是 `"="`，表示 stage2 的值传递到了 output，传递过程是正确的。问题在于 stage2 本身就是错的。

**第 5 步：继续追 stage2（因为它也是 Suspect，虽然它的子节点是 Ok）**

追到 stage1，发现 stage1 的值是 A（正确）。
追到 stage1 → stage2 的传递，发现 latency=1，但实际 stage2 的值不对。
→ **stage2 是 Suspect**

**最终结果：**
```
output @ 周期3 = B [RootCauseCandidate]
├── stage2 @ 周期2 = B [Ok]  ← 值传递正确
│   └── stage1 @ 周期1 = A [Suspect]  ← 但 stage1 是 A，stage2 应该是 A 却变成了 B！
│       └── stage0 @ 周期0 = A [Ok]
└── enable_out @ 周期3 = 1 [Ok]
```

结论：stage1 到 stage2 的传递出了问题。可能是 stage2 的寄存器逻辑有 bug。

---

## 四、完整工作流（7 步）

### Step 0：确认仿真状态

从 `run_summary.json` 确认仿真状态：
- `compile_failed` 或 `elab_failed` → 先修编译问题，不要进 BFS
- 只有 `assertion_failed` → 进入 BFS 流程

### Step 1：打开波形

```
open_waveform dump.vcd
list_signals dump.vcd  # 确认关键信号存在
```

### Step 2：解析断言日志

```
load_assertion_log transcript.log
```

输出示例：
```
Parsed events: 3
- Error assert_data_valid @ 1750 ns in tb_top.dut
- Warning assert_coeff_overflow @ 2100 ns in tb_top.dut.coeff
- Error assert_data_valid @ 3500 ns in tb_top.dut
```

你得到了 **断言名称、严重级别、失败时间、作用域**。

### Step 3：确定 BFS 入口信号

**方式 A：有 design_spec.yaml**
```
load_design_spec design_spec.yaml
```
然后在 spec 中查找断言名对应的 `observe_signals`，取第一个作为入口。

**方式 B：没有 design_spec.yaml**
```
suggest_entry_signals dump.vcd mydeps --assertion-name assert_data_valid --scope-path tb_top.dut
```

工具会返回按优先级排序的候选信号：
```
Suggested entry signals:
- TOP.data_o [T1:deps-output] [assertion-match] fan_in=3 types=sequential,control | Output signal with fan-in chain
- TOP.valid_flag [T1:deps-output] [assertion-match] fan_in=2 types=sequential | Matches assertion token 'valid'
- TOP.coeff_addr [T2:deps-boundary] fan_in=0 | In deps but no upstream traceable
```

**T1 > T2 > T3**，优先选 T1（有完整上游链可以追溯的信号）。

### Step 4：加载依赖图 + 验证入口

```
load_deps deps.yaml
find_fan_in mydeps TOP.data_o
```

确认：
- 信号别名解析正确（如 `clk` → `tb_top.dut.clk`）
- 延迟周期数与 RTL 一致
- 上游链完整

### Step 5：确认入口信号值

```
read_signal dump.vcd TOP.data_o --time-value 1750 --time-unit ns
```

确认失败时刻的信号值确实是异常的。

### Step 6：执行 BFS 追溯

```
trace_root_cause dump.vcd mydeps TOP.data_o --time-value 1750 --time-unit ns --max-depth 8
```

可选：如果有 spec，加上 `--spec-id myspec` 获取 stop_signals 提示。

### Step 7：分析结果

BFS 输出包含：
- **追溯树**：每个节点的信号名、时间、值、状态
- **根因候选列表**：按优先级排序

状态解读：
- `RootCauseCandidate` → 高优先级，所有上游正确但自身异常
- `Suspect` → 需要进一步检查
- `Ok` → 该段正常，排除
- `Boundary` → 调试边界，可能需要手动扩展 deps.yaml

---

## 五、适用场景

### 5.1 最适合的场景

| 场景 | 说明 |
|------|------|
| **流水线 bug** | 多级寄存器/流水线，数据在某个阶段出错 |
| **控制信号异常** | enable/select/gate 信号不正确导致数据错误 |
| **BRAM 读写问题** | 地址/使能/读数据之间的时序关系错误 |
| **协议握手失败** | valid/ready 握手时序不符合预期 |
| **回归测试失败** | 已知断言失败，快速定位根因 |

### 5.2 不太适合的场景

| 场景 | 原因 |
|------|------|
| **首次调试全新设计** | 还没有 deps.yaml，需要先建模 |
| **编译/综合错误** | 这不是波形级的问题 |
| **纯组合逻辑环路** | BFS 依赖时钟边沿回溯，纯组合逻辑没有时序参考 |
| **跨时钟域问题** | 当前版本在 CDC 边界停止，不支持跨域追溯 |

### 5.3 deps.yaml 的渐进式策略

**不要试图一次性建模整个设计。** 对于 10 万+ 行的 RTL，这是不可行的。

| 版本 | 信号数 | 策略 |
|------|--------|------|
| V1 | 5-10 | 只建失败输出信号 + 直接上游（1级寄存器 + 控制使能） |
| V2 | 10-20 | 添加失败路径上的直接上游寄存器级 |
| V3 | 20-50 | 添加 BRAM 读路径（memory 类型，延迟=2）、FSM 自环、流水线中间级 |
| V4 | 50-100 | 添加通道别名、CDC 边界标记 |

**每次断言失败只增加 5-10 个信号。** deps 图随着调试经验自然生长，不是一开始就全建好。

---

## 六、工具调用方式

wave-analyzer-mcp 提供了两种调用方式：

### 6.1 MCP 工具（AI Agent 调用）

通过 MCP 协议，AI Agent 可以按上述 7 步流程自动调用：

```python
# MCP 工具调用（伪代码）
result = mcp.call("trace_root_cause", {
    "waveform_id": "dump.vcd",
    "deps_id": "deps.yaml",
    "signal_path": "TOP.data_o",
    "time_value": 1750,
    "time_unit": "ns",
    "max_depth": 8,
    "spec_id": "design_spec.yaml"  # 可选
})
```

### 6.2 命令行工具（人工调试）

```bash
# 打开波形 + 追溯根因
waveform-cli open_waveform dump.vcd --alias mywave -- \
  load_deps deps.yaml --alias mydeps -- \
  trace_root_cause mywave mydeps TOP.data_o --time-value 1750 --time-unit ns
```

### 6.3 常用命令速查

| 命令 | 用途 |
|------|------|
| `open_waveform <file>` | 加载波形文件 |
| `list_signals <id>` | 列出信号，确认信号存在 |
| `read_signal <id> <path> --time-value N --time-unit ns` | 读某时刻的信号值 |
| `find_signal_events <id> <path>` | 查看信号的所有跳变事件 |
| `find_conditional_events <id> "条件表达式"` | 搜索满足条件的时刻 |
| `load_deps <file>` | 加载依赖图 |
| `find_fan_in <id> <path>` | 查看某信号的上游依赖 |
| `load_assertion_log <file>` | 解析断言日志 |
| `load_design_spec <file>` | 加载设计规格 |
| `suggest_entry_signals` | 推荐入口信号 |
| `trace_root_cause` | 执行 BFS 追溯 |
| `analyze_handshake` | 握手协议分析（新增） |
| `measure_signal` | 时钟/脉冲测量（新增） |

---

## 七、常见问题

### Q1: deps.yaml 建错了怎么办？

BFS 的结果质量完全取决于 deps.yaml 的准确性。如果追溯结果不符合预期，首先检查：
1. 依赖类型（type）是否正确
2. 延迟周期数（latency_cycles）是否与 RTL 一致
3. 检查表达式（check）是否符合预期

### Q2: 为什么会追到一个 RootCauseCandidate？

这是**好消息**。说明算法发现：这个信号的所有上游输入都正确，但它本身的值异常。
这意味着问题要么在这个信号本身的逻辑中（未建模的组合逻辑），要么在更上游。
下一步：检查这个信号的 RTL 实现，或者扩展 deps.yaml 添加更多上游。

### Q3: 追到 Boundary 就没法继续了吗？

Boundary 表示"当前 deps.yaml 中没有定义这个信号的上游"。你可以：
1. 在 deps.yaml 中添加该信号的上游依赖
2. 重新运行 BFS
3. 这就是渐进式建模的意义——每次遇到 Boundary，就知道该扩展哪里

### Q4: 时钟频率变了会影响追溯吗？

不会。BFS 使用的是**时钟边沿计数**，不是绝对时间。
无论时钟是 100MHz 还是 200MHz，`latency_cycles: 2` 都回溯 2 个时钟沿。

### Q5: 能不能同时追溯多个入口信号？

`trace_root_cause` 每次只接受一个入口信号。如果有多个入口：
1. 先追溯主入口（优先级最高的 T1 信号）
2. 分析结果后，再追溯其他入口
3. 或者用 `find_conditional_events` 做交叉检查

---

## 八、架构图

```
┌─────────────────────────────────────────────────────────┐
│                    wave-analyzer-mcp 架构                     │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐             │
│  │ assertion │  │   spec   │  │   deps   │             │
│  │  .rs     │  │   .rs    │  │   .rs    │             │
│  │          │  │          │  │          │             │
│  │ 断言解析  │  │ 规格加载  │  │ 依赖图    │             │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘             │
│       │              │              │                   │
│       │              ▼              │                   │
│       │       ┌──────────┐         │                   │
│       │       │  entry   │         │                   │
│       │       │  signal  │         │                   │
│       │       │  .rs     │         │                   │
│       │       └────┬─────┘         │                   │
│       │            │                │                   │
│       ▼            ▼                ▼                   │
│  ┌──────────────────────────────────────────┐          │
│  │              bfs.rs（核心引擎）            │          │
│  │                                          │          │
│  │  • 队列驱动的 BFS 遍历                    │          │
│  │  • 时钟边沿回溯（time_map.rs）             │          │
│  │  • 边级别检查表达式判定                    │          │
│  │  • 节点级别状态综合                        │          │
│  │  • 根因候选排序                            │          │
│  └──────────────────┬───────────────────────┘          │
│                     │                                   │
│  ┌──────────────────┴───────────────────────┐          │
│  │              waveform (wellen)            │          │
│  │                                          │          │
│  │  • VCD/FST 读取                           │          │
│  │  • 信号值查询                             │          │
│  │  • 时钟边沿提取                           │          │
│  └──────────────────────────────────────────┘          │
│                                                         │
│  ┌──────────────────────────────────────────┐          │
│  │           MCP Server / CLI                │          │
│  │                                          │          │
│  │  • stdio 模式（AI Agent 调用）             │          │
│  │  • HTTP 模式（远程服务）                   │          │
│  │  • CLI 模式（人工调试）                    │          │
│  └──────────────────────────────────────────┘          │
└─────────────────────────────────────────────────────────┘
```

---

## 九、总结

BFS 根因追溯算法的核心价值：

1. **自动化** — 替代人工在波形中逐个信号、逐个时间点排查的重复劳动
2. **结构化** — 通过 deps.yaml 将设计知识沉淀为可复用的依赖图
3. **渐进式** — 从 5 个信号开始，每次失败只加 5-10 个，deps 图自然生长
4. **时钟精确** — 基于时钟边沿回溯，不是简单的时间索引减法
5. **状态判定** — 8 种节点状态，自动区分"上游正确"、"有问题"、"到边界"

**一句话总结：**
> BFS 算法从断言失败的信号出发，沿着依赖图往回追溯，在每个节点读取波形实际值并做一致性检查，最终自动标记出"所有上游都正确但自身异常"的根因候选信号，大幅缩短 RTL 调试的排查时间。
