# 信号依赖图格式规范

## 1. 目标

`deps.yaml` 是 BFS 根因分析的结构化输入，用于描述：

1. 某个输出/状态信号依赖哪些上游信号。
2. 这些依赖属于组合、时序、存储器、握手还是边界类关系。
3. 发生失败时，如何按参考时钟在波形中回退到正确的观察时刻。

---

## 2. 基本原则

### 2.1 图类型

依赖图在本项目中定义为：

```text
Directed Graph（一般有向图）
```

不是严格 DAG。

原因：

1. 计数器、状态机、流水寄存器常有“自身上一拍”依赖。
2. 握手/反压链可能形成环。
3. 设计中存在时序反馈是正常情况。

因此 BFS 必须显式支持循环检测，而不是假定图无环。

### 2.2 建图粒度

| 推荐粒度 | 说明 |
|---|---|
| 模块边界信号 | 适合快速覆盖主链路 |
| 关键内部寄存级 | 适合流水线/状态机/BRAM 路径 |
| 关键控制信号 | 如 `valid/ready/en/sel/state` |

不建议把所有组合中间线网都建进去，否则维护成本过高。

### 2.3 与 spec 的边界

| 文件 | 负责内容 |
|---|---|
| `design_spec.yaml` | 定义需求、行为、断言入口 |
| `deps.yaml` | 定义失败后往上游怎么追 |

---

## 3. 时间语义

### 3.1 统一定义

所有时序类依赖统一使用以下字段：

| 字段 | 含义 |
|---|---|
| `clock` | 依赖边所属参考时钟逻辑名，必须与 `design_spec.yaml.clock_domains[].name` 一致 |
| `edge` | `posedge` 或 `negedge` |
| `latency_cycles` | 相对于该参考时钟回退的周期数 |

### 3.2 正确追溯方式

对时序边：

1. 先根据逻辑时钟名解析到实际波形时钟，再找到当前节点对应的最近有效边沿。
2. 再回退 `latency_cycles` 个边沿。
3. 在该时刻附近读取上游信号值。

### 3.3 明确禁止

以下方式是错误的，不得写入实现：

```text
dep_time_index = current_time_index - latency_cycles
```

因为 VCD/FST 的 `time index` 不是“时钟拍号”。

---

## 4. 顶层结构

```yaml
format_version: "1.0"
description: "<描述>"

signal_aliases:
  - canonical: "<规范路径>"
    modelsim: "<ModelSim 波形中的实际路径>"
    vivado: "<可选，Vivado 仿真路径>"

clock_aliases:
  - clock_name: "<逻辑时钟名>"
    modelsim: "<ModelSim 波形中的实际时钟路径>"
    vivado: "<可选，Vivado 仿真路径>"

dependencies:
  - output: "<被分析信号>"
    category: data | control | state | memory | protocol
    description: "<可选描述>"
    depends_on:
      - signal: "<上游信号>"
        type: combinational | sequential | memory | control | protocol | boundary
        description: "<可选描述>"
        logic_type: or | and | nand | nor | xor | mux | null
        clock: "<逻辑时钟名或null>"
        edge: posedge | negedge | null
        latency_cycles: 0
        protocol_kind: handshake | backpressure | none
        boundary_kind: input_port | constant | cdc | blackbox | manual_stop
        check: "=" | "!=" | ">0" | "==0" | null
        condition_expression: "<可选：复合条件表达式>"
```

---

## 5. 依赖类型语义

| 类型 | 含义 | 时间处理 |
|---|---|---|
| `combinational` | 同一观察时刻的组合依赖 | 不回退周期 |
| `sequential` | 经过寄存器/流水线的时序依赖 | 按 `clock + latency_cycles` 回退 |
| `memory` | 地址/使能到读数据的时序依赖 | 按 `clock + latency_cycles` 回退 |
| `control` | 使能/选择/状态控制关系 | 默认同观察时刻，也可配 `clock` |
| `protocol` | `valid/ready` 等协议关系 | 默认同观察时刻，也可配 `clock` |
| `boundary` | 调试边界，不继续自动展开 | 到此停止或人工接管 |

### 5.1 `boundary` 的意义

`boundary` 是新增的重要类型，适用于：

1. 顶层输入端口。
2. 常量配置源。
3. CDC 边界。
4. 外部 IP 黑盒边界。
5. 目前不打算自动展开的复杂块。

这比把所有无上游节点都简单叫作 `leaf` 更可控。

---

## 6. 推荐字段解释

### 6.1 `check`

`check` 只做轻量边一致性判断，不承担完整逻辑证明。

| 取值 | 含义 |
|---|---|
| `=` | 下游值应等于该上游值 |
| `!=` | 下游值应不同于该上游值 |
| `>0` | 上游应为非零 |
| `==0` | 上游应为零 |

适用范围：

1. 寄存级直通。
2. valid/enable 的简单门控。
3. BRAM 地址/使能的基本健康检查。

不适用：

1. 复杂算术逻辑。
2. 多输入逻辑函数求值。
3. X/Z 语义严格比较。

### 6.5 `logic_type`

`logic_type` 用于标注组合边的逻辑语义，使 BFS hint 推理更准确。当未指定时，BFS
只能从单边值推断（ambiguous logic），容易误标 OR 逻辑中不贡献的输入为 Suspect。

| 取值 | 含义 | BFS 分类规则 |
|---|---|---|
| `or` | OR 门：任一输入=1 → 输出=1 | input=1, output=0 → Suspect；input=0, output=1 → Context |
| `and` | AND 门：所有输入=1 → 输出=1 | input=0, output=1 → Suspect；input=1, output=0 → Context |
| `nand` | NAND 门：AND 取反 | 同 AND 但输出值反转判定 |
| `nor` | NOR 门：OR 取反 | 同 OR 但输出值反转判定 |
| `xor` | XOR 门 | 单边分析不确定 → Context |
| `mux` | 多路选择器 | 未选通输入 → Context |

仅适用于 1-bit 组合边（`type: combinational`）。

示例：

```yaml
- output: "o_power_off"
  depends_on:
    - signal: "v_protect"
      type: combinational
      logic_type: or
    - signal: "c_protect"
      type: combinational
      logic_type: or
    - signal: "t_protect"
      type: combinational
      logic_type: or
```

当 `o_power_off=0` 而 `v_protect=1` 时，OR 逻辑下标记为 **Suspect**（input=1 但
output=0 是矛盾）；当 `c_protect=0` 时，标记为 **Context**（其他输入可能满足 OR）。

### 6.4 `condition_expression`

`condition_expression` 是 `check` 的增强替代，用于描述完整的控制条件逻辑。

| 优先级 | 说明 |
|---|---|
| `condition_expression` 存在且可评估 | 使用条件引擎评估，忽略 `check` |
| `condition_expression` 评估失败 | 回退到 `check` 字段 |
| `condition_expression` 不存在 | 使用 `check` 字段 |

语法支持（LALRPOP 条件语法）：

- 逻辑运算：`&&`（AND）、`||`（OR）、`!`（NOT）
- 位运算：`&`（AND）、`|`（OR）、`^`（XOR）、`~`（NOT）
- 比较：`==`（等于）、`!=`（不等于）
- 位提取：`signal[msb:lsb]`、`signal[bit]`
- Verilog 常量：`4'b0101`、`3'd5`、`8'hFF`
- 时间回溯：`$past(signal)` — 取上一时刻的信号值
- 括号分组

信号路径使用 canonical 名称（如 `TOP.enable`），与 deps.yaml 其他字段一致。

示例：

```yaml
- signal: "TOP.ch0.output_enable"
  type: control
  clock: clk_sys
  edge: posedge
  latency_cycles: 0
  condition_expression: "TOP.ch0.state == 2'b01 && TOP.ch0.valid != 1'b0"
  check: ">0"  # 回退值，当 condition_expression 评估失败时使用
```

简单条件的 `check` 推导规则：

| condition_expression | 推导 check |
|---|---|
| `TOP.enable`（裸信号） | `>0` |
| `!(TOP.enable)` 或 `!TOP.enable` | `==0` |
| `~TOP.signal` | `==0` |
| 其他复合表达式 | null（仅依赖 condition_expression） |

### 6.2 `signal_aliases`

相控阵和 generate 结构常导致波形路径与逻辑命名不一致，因此：

1. `dependencies` 中统一写 canonical 名称。
2. 运行时由 `simulator=modelsim` 解析到实际波形路径。
3. `clock` 字段不要混写成 `TOP.clk_sys` 这类波形路径；统一写逻辑时钟名，如 `clk_sys`，再由运行时解析到实际波形时钟。

### 6.3 `clock_aliases`

为避免 `clock=clk_sys` 这类逻辑名在运行时无法落到真实波形路径，推荐显式提供：

1. `clock_aliases[].clock_name`：逻辑时钟名，必须与 `design_spec.yaml.clock_domains[].name` 一致。
2. `clock_aliases[].modelsim`：该时钟在 ModelSim 波形中的实际路径。
3. 若存在多仿真器，可继续扩展 `vivado` 等字段。

推荐解析顺序：

```text
dep.clock
  -> clock_aliases[clock_name]
  -> simulator-specific waveform path
  -> waveform signal lookup
```

若缺少对应 `clock_aliases`，应返回显式错误，而不是回退成模糊搜索。

---

## 7. 推荐模板

```yaml
format_version: "1.0"
description: "beam_ctrl 关键数据与控制依赖图"

signal_aliases:
  - canonical: "TOP.ch0.beam_data_o"
    modelsim: "TOP.gen_ch__0.beam_data_o"
  - canonical: "TOP.ch0.coeff_valid"
    modelsim: "TOP.gen_ch__0.coeff_valid"
  - canonical: "TOP.cfg_valid"
    modelsim: "TOP.cfg_valid"

clock_aliases:
  - clock_name: "clk_sys"
    modelsim: "TOP.clk_sys"

dependencies:
  - output: "TOP.ch0.beam_data_o"
    category: data
    description: "通道 0 波束输出"
    depends_on:
      - signal: "TOP.ch0.data_pipe3"
        type: sequential
        clock: "clk_sys"
        edge: posedge
        latency_cycles: 1
        check: "="
      - signal: "TOP.ch0.output_enable"
        type: control
        clock: "clk_sys"
        edge: posedge
        latency_cycles: 0
        check: ">0"

  - output: "TOP.ch0.data_pipe3"
    category: data
    depends_on:
      - signal: "TOP.ch0.data_pipe2"
        type: sequential
        clock: "clk_sys"
        edge: posedge
        latency_cycles: 1
        check: "="
      - signal: "TOP.ch0.coeff_valid"
        type: control
        clock: "clk_sys"
        edge: posedge
        latency_cycles: 0
        check: ">0"

  - output: "TOP.ch0.coeff_valid"
    category: control
    depends_on:
      - signal: "TOP.cfg_valid"
        type: sequential
        clock: "clk_sys"
        edge: posedge
        latency_cycles: 3
        check: "="

  - output: "TOP.cfg_valid"
    category: control
    depends_on:
      - signal: "TOP.cfg_valid"
        type: boundary
        boundary_kind: input_port
        clock: null
        edge: null
        latency_cycles: 0
        check: null
```

---

## 8. 常见模式

### 8.1 单级寄存器

```yaml
- output: "TOP.stage2.data_o"
  category: data
  depends_on:
    - signal: "TOP.stage1.data_o"
      type: sequential
      clock: "clk_sys"
      edge: posedge
      latency_cycles: 1
      check: "="
```

### 8.2 Counter / FSM 自回馈

```yaml
- output: "TOP.state_reg"
  category: state
  depends_on:
    - signal: "TOP.state_reg"
      type: sequential
      clock: "clk_sys"
      edge: posedge
      latency_cycles: 1
      check: null
    - signal: "TOP.next_state"
      type: combinational
      clock: null
      edge: null
      latency_cycles: 0
      check: null
```

这类模式证明图不是 DAG，BFS 必须有循环保护。

### 8.3 BRAM 读取

```yaml
- output: "TOP.coeff_rd_data"
  category: memory
  depends_on:
    - signal: "TOP.coeff_rd_addr"
      type: memory
      clock: "clk_sys"
      edge: posedge
      latency_cycles: 2
      check: null
    - signal: "TOP.coeff_rd_en"
      type: control
      clock: "clk_sys"
      edge: posedge
      latency_cycles: 0
      check: ">0"
```

### 8.4 CDC 边界

```yaml
- output: "TOP.sys_domain.sync_flag"
  category: control
  depends_on:
    - signal: "TOP.rf_domain.async_flag"
      type: boundary
      boundary_kind: cdc
      clock: null
      edge: null
      latency_cycles: 0
      check: null
```

当前版本只定位到 CDC 边界，不做自动跨域展开。

---

## 9. 构建建议

| 步骤 | 建议 |
|---|---|
| 第 1 版 | 先覆盖顶层关键输出、关键状态、关键握手 |
| 第 2 版 | 加入关键流水寄存级 |
| 第 3 版 | 补齐 BRAM/LUT/系数链路 |
| 第 4 版 | 处理 generate 通道别名、CDC 边界、黑盒边界 |

---

## 10. 校验建议

在实现 `find_fan_in` / `find_fan_out` 后，建议至少验证以下内容：

1. 图能正确展开到预期上游。
2. 自回馈节点不会导致无限循环。
3. 别名解析后的路径能在 ModelSim 波形中找到。
4. 关键路径的 `clock` 与 `latency_cycles` 与 RTL 一致。

---

## 11. 与 BFS 的直接契约

`deps.yaml` 对 BFS 的最小契约如下：

| 项目 | 要求 |
|---|---|
| 信号名 | 使用 canonical 名称 |
| 时间语义 | 使用 `clock + edge + latency_cycles` |
| 边界节点 | 用 `boundary` 明确标出 |
| 循环 | 允许存在，BFS 负责检测 |

如果这些契约不满足，BFS 输出只能是“疑点树”，不能稳定收敛为高可信根因。
