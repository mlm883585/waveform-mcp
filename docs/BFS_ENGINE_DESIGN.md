# BFS 根因分析引擎设计

## 1. 目标

给定一个失败行为对应的入口信号和故障时刻，基于 `deps.yaml` 与波形文件自动生成一棵“问题传播树”，帮助工程师和 AI 快速定位：

1. 错误首先出现在哪一级。
2. 是数据路径问题、控制问题、存储器时序问题还是边界问题。
3. 哪些节点只是上下文，哪些节点最值得优先检查。

---

## 2. 输入与输出

### 2.1 输入

| 输入项 | 来源 |
|---|---|
| `waveform_id` | 已打开的波形 |
| `signal_path` | `design_spec.yaml` 或人工指定的入口信号 |
| `time_anchor` | 故障时间锚点。设计语义上可来源于 transcript 时间值或波形时间索引；第一版接口固定使用 `time_index`，时间值到索引的映射由上层先完成 |
| `deps_alias` | 已加载依赖图别名 |
| `options` | `max_depth`、`stop_signals`、`enable_auto_check` 等 |

### 2.2 输出

| 输出项 | 说明 |
|---|---|
| BFS 树 | 节点、边、时间点、状态、值 |
| 候选根因列表 | 按优先级排序的重点节点 |
| 调试摘要 | 给 AI/用户的文本摘要 |

---

## 3. 设计修正

### 3.1 图不是 DAG

本引擎面向一般有向图，不假设无环。

必须支持：

1. 自回馈寄存器。
2. FSM 状态自依赖。
3. 握手控制环。

因此循环检测是必选能力，不是补充能力。

### 3.2 时间模型不是 `time_index - N`

本引擎的时间回退必须按参考时钟边沿进行，而不是按波形采样索引直接减法。

原因：

1. 事件驱动波形中的时间表并非等间隔时钟采样。
2. 断言失败时间往往是物理时间值，如 `1750 ns`。
3. 寄存器语义必须绑定到参考时钟。

---

## 4. 节点模型

建议节点结构包含如下信息：

```rust
struct BfsNode {
    canonical_signal: String,
    resolved_signal: String,
    time_index: usize,
    time_ps: u64,
    depth: usize,
    status: NodeStatus,
    actual_value: Option<String>,
    expected_hint: Option<String>,
    edge_type: Option<String>,
    clock: Option<String>,
    latency_cycles: Option<u32>,
    note: Option<String>,
}
```

### 4.1 节点状态

| 状态 | 含义 |
|---|---|
| `Suspect` | 当前节点可疑，需要继续追 |
| `Ok` | 与边检查结果一致 |
| `Boundary` | 已到输入/CDC/黑盒等边界 |
| `Stopped` | 命中用户停止点 |
| `Truncated` | 达到深度限制 |
| `Cyclic` | 发现循环并截断 |
| `Context` | 仅作上下文展示 |
| `RootCauseCandidate` | 高优先级候选根因 |

---

## 5. 时间映射算法

### 5.1 统一步骤

对每一条时序类依赖边：

1. 读取该边的 `clock`、`edge`、`latency_cycles`。
2. 在波形中找到参考时钟信号。
3. 以当前节点的 `time_ps` 为基准，找到最近一个不晚于当前时刻的目标边沿。
4. 沿时钟边沿列表回退 `latency_cycles` 次。
5. 将回退后的时刻再映射到上游节点读取时间。

### 5.2 结果

```text
child_time = previous_clock_edge(current_time, clock, edge, latency_cycles)
```

而不是：

```text
child_time_index = current_time_index - latency_cycles
```

### 5.3 边沿表构建

边沿表是时间映射的基础数据结构，必须在 BFS 开始前构建。

**构建步骤:**

1. 从 `dep.clock` 经 `clock_aliases` 解析到实际波形时钟路径（如 `TOP.clk_sys`）。
2. 在波形 hierarchy 中查找该时钟信号对应的 `SignalRef`。
3. 调用 `waveform.load_signals(&[signal_ref])` 加载时钟信号数据。
4. 调用 `find_signal_events` 获取该时钟信号的全部变化事件。
5. 对事件列表按 `edge` 字段过滤：
   - `edge=posedge`：只保留从 `0` → `1` 的变化事件。
   - `edge=negedge`：只保留从 `1` → `0` 的变化事件。
6. 按时间排序，生成 `Vec<(time_index, time_value)>` 边沿索引表。

**边沿表结构:**

```rust
struct ClockEdgeTable {
    clock_name: String,
    resolved_path: String,
    edge: ClockEdge,  // Posedge | Negedge
    edges: Vec<ClockEdgeEntry>,
}

struct ClockEdgeEntry {
    time_index: usize,
    time_value: u64,  // 原始时间值（配合 timescale 转换为 ps）
}
```

**posedge/negedge 判定规则:**

对时钟信号的连续变化事件 `(t0, val0)` → `(t1, val1)`：
- `posedge`：`val0 == 0 && val1 == 1`，边沿时刻为 `t1`。
- `negedge`：`val0 == 1 && val1 == 0`，边沿时刻为 `t1`。

对于多 bit 信号误用为时钟（如总线），应返回错误而非强行取最高位。

### 5.4 `latency_cycles=0` 的语义

不同依赖边类型下 `latency_cycles=0` 的含义不同：

| 边类型 | `latency_cycles=0` 含义 | 时间处理 |
|---|---|---|
| `combinational` | 同观察时刻，不回退 | 直接在 `node.time_index` 读取上游 |
| `control` + `latency_cycles=0` | 同观察时刻，不回退 | 直接在 `node.time_index` 读取上游 |
| `sequential` + `latency_cycles=0` | 最近参考时钟边沿（不回退） | 找到最近一个不晚于当前时刻的时钟边沿，在该 `time_index` 读取上游 |
| `memory` + `latency_cycles=0` | 最近参考时钟边沿（不回退） | 同上 |
| `boundary` | 不做时间回退 | 直接在 `node.time_index` 读取 |

**关键区分:** `control` 类边即使配了 `clock` 字段，`latency_cycles=0` 仍意为"同观察时刻"，因为控制信号对数据的影响通常是组合逻辑门控。而 `sequential/memory` 类边 `latency_cycles=0` 意为"对齐到最近时钟边沿"，因为寄存器和存储器的语义始终绑定到时钟边沿。

### 5.5 上游取样规则

回退到目标时钟边沿后，读取上游信号值的规则：

1. 获取目标边沿对应的 `time_index`。
2. 直接在该 `time_index` 处读取上游信号值。
3. 如果该 `time_index` 处上游信号无事件记录（信号未变化），wellen 会自动返回最近一次有效值。
4. 不做"向前搜索最近变化事件"的额外处理——wellen 的信号读取 API 已内置此语义。

### 5.6 边界情况

| 情况 | 处理方式 |
|---|---|
| 找不到参考时钟 | 节点标记为 `Boundary` 或 `Stopped`，提示依赖图不完整 |
| `clock_aliases` 中缺少对应逻辑时钟名 | 返回结构化错误 `CLOCK_NOT_FOUND`，不回退成模糊搜索 |
| 回退超出波形起点 | 截断到最早有效边沿，并标记注释 |
| 控制/协议边未给出时钟 | 默认沿当前观察时刻读取 |
| 时钟信号为多 bit 信号 | 返回错误，不强行取最高位做边沿判定 |
| 边沿表为空（时钟无任何变化） | 节点标记为 `Boundary`，提示时钟可能未正确 dump |

---

## 6. 主算法

```text
FUNCTION trace_root_cause(target_signal, fail_time, graph, waveform, options):

  root = build_root_node(target_signal, fail_time)
  queue = [root]
  visited = Set()
  tree = []

  WHILE queue not empty:
    node = pop_front(queue)
    key = (node.canonical_signal, node.time_index)

    IF key in visited:
      node.status = Cyclic
      append(tree, node)
      CONTINUE
    visited.add(key)

    node.actual_value = read_waveform_value(node.resolved_signal, node.time_index)

    IF node.canonical_signal in options.stop_signals:
      node.status = Stopped
      append(tree, node)
      CONTINUE

    IF node.depth >= options.max_depth:
      node.status = Truncated
      append(tree, node)
      CONTINUE

    deps = graph.fan_in(node.canonical_signal)
    IF deps is empty:
      node.status = Boundary
      append(tree, node)
      CONTINUE

    children = []
    FOR dep in deps:
      dep_time = resolve_dep_time(node, dep, waveform)
      child = build_child_node(dep, dep_time)
      child.actual_value = read_waveform_value(child.resolved_signal, child.time_index)
      child.status = evaluate_edge_status(node, child, dep, options)
      children.add(child)

    node.status = summarize_node_status(node, children)
    append(tree, node)

    FOR child in children:
      IF should_expand(child):
        push_back(queue, child)

  RETURN summarize_tree(tree)
```

---

## 7. 边检查策略

### 7.1 `evaluate_edge_status`

建议采用轻量规则：

| 条件 | 结果 |
|---|---|
| `check` 存在且通过 | `Ok` |
| `check` 存在但失败 | `Suspect` |
| `boundary` 类型 | `Boundary` |
| 无 `check` 但属于主要数据路径 | `Context` 或 `Suspect` |

### 7.3 `check` 字段评估的实现机制

`check` 字段评估使用简化的 `BigUint` 直接比较，**不走 LALRPOP 条件引擎**。

**原因:**

1. LALRPOP 条件引擎是面向复杂布尔/位运算表达式的设计，而 `check` 只需做 4 种简单比较。
2. 条件引擎需要信号缓存和多步求值，BFS 的边检查只需读两个信号值做一次比较。
3. 直接用 `BigUint` 比较更可控、更易测试。

**评估规则:**

| `check` 值 | 评估逻辑 | 通过条件 |
|---|---|---|
| `=` | `parent_value == child_value`（高位补零对齐后） | 两值相等 |
| `!=` | `parent_value != child_value`（高位补零对齐后） | 两值不等 |
| `>0` | `child_value > BigUint::from(0u32)` | 上游信号非零 |
| `==0` | `child_value == BigUint::from(0u32)` | 上游信号为零 |
| `null` | 不做自动检查 | 边状态由 `summarize_node_status` 推断 |

**高位补零对齐规则:**

当 `parent_value` 和 `child_value` 位宽不同时：

1. 分别从波形获取两个信号的实际值（`SignalValue → BigUint`）。
2. 不需要额外对齐——`BigUint` 比较本身就是数学值比较。
3. 但需要注意：如果上游是 8 bit 信号值为 `8'hFF`（= 255），下游是 16 bit 信号值为 `16'h00FF`（= 255），`check: "="` 应判定为通过，因为数学值相等。

**特殊情况:**

| 情况 | 处理 |
|---|---|
| 信号值包含 X/Z | `check` 比较不可靠，应返回 `Context` 而非强行判断；X/Z 检出应优先由 SVA 负责 |
| 上游信号在目标时刻无数据 | `check` 无法执行，边状态设为 `Context` |
| `control` 类边但 `check=null` | 默认为 `Context`，不做 Suspect 推断 |

### 7.4 `summarize_node_status`

推荐规则：

1. 若所有关键子边都 `Ok`，而当前节点仍对应失败入口，可把当前节点提升为 `RootCauseCandidate`。
2. 若存在 `Boundary` 且该边是已知调试边界，则标记“需要人工继续”。
3. 若多个子边同时 `Suspect`，保留并行分支，不要过早剪枝。

---

## 8. 根因候选评分

建议不要把“根因”定义成唯一结论，而是输出候选集合。

### 8.1 候选评分因素

| 因素 | 解释 |
|---|---|
| 深度较浅 | 越靠近失败入口越容易先验证 |
| 子边大多 `Ok` | 当前节点内部出错概率更高 |
| 命中关键控制信号 | `enable/valid/state/addr` 常是高价值疑点 |
| 命中 spec 的 `stop_signals` 或 `entry_points` | 有更强语义支持 |
| 命中 `boundary` | 说明自动分析到边界，需要人工判断 |

### 8.2 输出形式

```text
Top Candidates:
1. TOP.ch0.coeff_valid @ 1750ns
2. TOP.ctrl.output_enable @ 1750ns
3. TOP.coeff_rd_addr @ 1740ns
```

---

## 9. Transcript 到 BFS 的标准链路

### 9.1 解析结果

断言日志解析应输出类似结构：

```rust
struct AssertionEvent {
    assertion_name: String,
    severity: String,
    scope_path: String,
    time_value: u64,
    time_unit: String,
    source_file: Option<String>,
    source_line: Option<u32>,
}
```

### 9.2 关键约束

Transcript 不应被设计成“直接包含失败信号路径”的可信来源。

正确链路应为：

1. 解析 transcript，拿到断言名和时间。
2. 去 `design_spec.yaml.assertions[].observe_signals` 找入口信号。
3. 再把时间值映射为波形观察点 `time_index`。
4. 调用 `trace_root_cause`。

这样链路稳定且可审计。

---

## 10. MCP / CLI 接口建议

### 10.1 MCP 工具

| 工具 | 作用 |
|---|---|
| `load_dependencies` | 加载 `deps.yaml` |
| `parse_assertion_log` | 解析 transcript，输出断言事件 |
| `trace_root_cause` | 对单个入口信号做 BFS |
| `find_fan_in` | 只查询依赖图 |
| `find_fan_out` | 只查询依赖图 |

### 10.2 注意

上述接口均已实现。接口契约详见 `INTERFACE_CONTRACTS.md`。

---

## 11. 适用于相控阵项目的调试模式

### 11.1 优先入口信号

建议优先支持以下类别：

| 类别 | 示例 |
|---|---|
| 通道输出数据 | `beam_data_o`, `phase_data_o` |
| 通道有效信号 | `valid_o`, `data_en` |
| 系数链路 | `coeff_addr`, `coeff_valid`, `coeff_rd_data` |
| 控制状态 | `state_reg`, `mode_sel`, `frame_sync` |
| 握手链路 | `ready_o`, `valid_i` |

### 11.2 多通道策略

对 16/32/64 通道结构，不建议一次性对全通道做 BFS 深追。

建议：

1. 先对出现故障的单个通道实例做定位。
2. 再对“同一类失败”的通道做聚合分析。
3. 共享 canonical 命名与 alias 模板。

---

## 12. 已知边界

| 边界 | 说明 |
|---|---|
| 复杂算术逻辑 | BFS 只能提示相关输入，不做代数求值 |
| CDC | 当前版本只定位到边界，不做跨域自动推理 |
| 黑盒 IP | 需在 `deps.yaml` 用 `boundary` 标识 |
| X/Z 语义 | 不宜依赖 BFS 轻量比较，应优先由 SVA 检出 |

---

## 13. 与其他文档的关系

| 文档 | 关系 |
|---|---|
| `WORKFLOW_DESIGN.md` | 规定 BFS 在总流程中的位置 |
| `DEPS_FORMAT.md` | 规定依赖图字段与时间语义 |
| `DESIGN_SPEC_FORMAT.md` | 规定断言失败后如何拿到入口信号 |

若三者冲突，以“时钟驱动时间回退、断言名映射入口信号、图允许有环”这三条为最高优先级。
