---
name: wave-analyzer
description: Use when analyzing VCD/FST waveform files, validating Verilog simulation results, debugging signal timing, tracing BFS dependencies, or checking UART/SPI/AXI/FSM/FIFO digital designs with wave-analyzer-cli.
---

# wave-analyzer — 统一波形分析技能

## CLI 路径

`wave-analyzer-cli.exe` 内嵌在本 skill 中。构造命令时，先定位当前 `SKILL.md` 所在目录，再拼接其 `bin` 子目录：

```
<skill目录>\bin\wave-analyzer-cli.exe
```

示例：如果当前 skill 目录是 `E:\fpgaProjectTest\interface_mmic_spi\test_vcd\.qwen\skills\wave-analyzer`，则：
```
E:\fpgaProjectTest\interface_mmic_spi\test_vcd\.qwen\skills\wave-analyzer\bin\wave-analyzer-cli.exe
```

在本仓库的示例目录中，该路径为：

```
example-skills\skills\wave-analyzer\bin\wave-analyzer-cli.exe
```

**下文用 `<CLI>` 指代解析后的绝对路径。**

### Windows 命令封装规则

在 Windows 上生成命令时，**不要把工具面板里的 `Shell` 标签写进命令正文**。`Shell "E:\...\wave-analyzer-cli.exe" ...` 不是合法命令。

PowerShell 必须使用调用操作符 `&`：

```powershell
& "E:\fpgaProjectTest\interface_mmic_spi\test_vcd\.qwen\skills\wave-analyzer\bin\wave-analyzer-cli.exe" `
  extract_deps "E:\fpgaProjectTest\interface_mmic_spi\test_vcd\rtl" interface_mmic_spi `
  --output "E:\fpgaProjectTest\interface_mmic_spi\test_vcd\deps.yaml"
```

cmd.exe 使用整条一行命令：

```bat
"E:\fpgaProjectTest\interface_mmic_spi\test_vcd\.qwen\skills\wave-analyzer\bin\wave-analyzer-cli.exe" extract_deps "E:\fpgaProjectTest\interface_mmic_spi\test_vcd\rtl" interface_mmic_spi --output "E:\fpgaProjectTest\interface_mmic_spi\test_vcd\deps.yaml"
```

不要输出被截断的参数，例如 `--ou…`。如果命令太长，PowerShell 用反引号换行；cmd 用一整行或切换到项目根目录后使用相对路径。

## ⛔ 强制规则（必须遵守）

### BFS 输出解读规则

BFS 工具提供的是**静态电路拓扑信息**，不是动态时序仿真结果。

**规则 1**：`find_fan_in` 的 `combinational, lat=0` ≠ "同拍生效"
- 只表示 A 是 B 寄存器 **next-state 逻辑**的组合输入
- Verilog NBA 语义下，B 的值在**下一个时钟沿**才更新

**规则 2**：`trace_root_cause` 的信号值不是同一拍快照
- 每个信号读的是它**最后一次值变化**的时刻，不同信号可能来自不同时间点
- 获取同一拍快照必须用 `read_signal --time-indices` 统一读取

**规则 3**：BFS 推断必须用直接信号测量交叉验证
- ⛔ 永远不要仅凭 BFS 依赖图分析就报告数据通路 bug
- 必须直接读取 FIFO dout / 移位寄存器 / 总线输出的实际值
- **如果直接测量与 BFS 推断矛盾，以测量为准**

**规则 4**：理解设计意图再判定
- 工具不能区分"正确的 CTL=0x00"和"stale 垃圾 0x00"
- 必须先读源码理解 FSM 行为，再用测量数据验证

### 数据通路交叉验证清单

**在将任何 ❌ 写入报告前，必须完成以下验证**：

| 验证对象 | 必须直接读取的信号 | 验证方法 |
|---|---|---|
| FIFO 写入 | `fifo.din` + `fifo.wr_en` 同一组 time_indices | 确认写入字节数和内容 |
| FIFO 读出 | `fifo.dout` + `fifo.rd_en` 同一组 time_indices | 确认读出字节数和内容，与写入逐字节对比 |
| 移位寄存器 | `r_xxx_shift` 在每个加载/移位时刻 | 追踪完整构建过程 |
| 总线输出 | 实际总线信号在每个采样沿 | 与移位寄存器内容逐 bit 比对 |

### 读帧/应答强制验证

当用户目标、RTL 名称、状态名或信号名涉及 `read`、`rd`、`resp`、`ack`、`MISO`、`rx_data`、`rx_valid`、`MMIC`、"应答"、"读帧"时，必须执行读帧应答专项验证。

不能只验证 `cs_n`、`spi_clk`、`mosi`、`tx_data` 后就报告 SPI 正常。读帧功能的最低证据链是：

1. 读请求/读命令确实发出
2. 读命令后仍有足够时钟采样返回数据
3. `miso/rx_shift` 有被采样的返回位
4. `rx_valid/resp_valid/ack/done` 在期望窗口内出现，或 `timeout/no_resp/error` 置位
5. `rx_data/resp_data` 与 MISO 位流一致

如果第 1 项成立但第 3/4 项缺失，必须报告"读帧无应答"或"响应证据缺失"；不能写"未发现问题"。

如果 testbench 没有驱动 MISO 或没有 MMIC 响应模型，报告为"仿真环境未提供应答"，并列出缺失的应答信号/驱动证据。

## 执行流程

### 步骤 -1：环境健康检查（首次使用时）

```powershell
& "<CLI>" check_env
```

如果输出包含错误（如 VC++ Runtime not found），提示用户：
```
winget install Microsoft.VCRedist.2015+.x64
```

如果 `extract_deps` 需要 pyverilog 但未安装，提示：
```
pip install pyverilog
```

### 步骤 0：读取 RTL 源码（理解设计）

**必须先读取 Verilog 源码**，提取：
1. 状态机定义（localparam/parameter 状态编码）
2. 时序锚点信号（分频计数器、tick/pulse）
3. 握手/控制信号（valid, ready, done, busy, sof, eop）
4. 数据通路信号（shift, data, cnt, fifo.din/dout）
5. 设计模式识别：UART / SPI / AXI / FSM / FIFO

根据源码分析，查阅 [reference.md](reference.md) 选择对应锚点信号。

### 步骤 1：打开波形 + 加载 deps + 读层次

**关键：CLI 是无状态的，每次调用必须用 `--` 链式执行所有命令。**

⛔ **禁止跨 shell 调用复用 alias**：
- `wave1` / `deps1` 只在当前 `wave-analyzer-cli.exe` 进程内有效
- 下一次 shell 调用里，之前的 `wave1` / `deps1` 已经不存在
- 任何使用 `wave1` 的命令，必须在同一个命令行前面包含 `open_waveform <vcd_path> --alias wave1`
- 任何使用 `deps1` 的命令，必须在同一个命令行前面包含 `load_deps <deps.yaml> --alias deps1`
- 如果发现输出类似 "waveform not found"、"unknown alias"、"deps not found"，不要继续分析；重写为链式命令后重跑

✅ 正确模式：

```bat
<CLI> open_waveform <vcd_path> --alias wave1 ^
  -- load_deps deps.yaml --alias deps1 ^
  -- find_signal_events wave1 "<signal>" --limit 20 ^
  -- multi_signal_timeline wave1 --signals "<sig1>,<sig2>" --start <s> --end <e> --limit 50 --format hex
```

❌ 错误模式：

```bat
<CLI> open_waveform <vcd_path> --alias wave1
<CLI> find_signal_events wave1 "<signal>" --limit 20
```

第二行是新进程，`wave1` 不存在；这种输出不能用于报告结论。

### 命令生成自检门

在运行任何 `<CLI>` 命令前，先逐项检查；任一项不满足，必须改写命令后再执行：

| 检查项 | 通过条件 |
|---|---|
| alias 自包含 | 命令里使用 `wave1` 时，同一命令行前面已有 `open_waveform <vcd_path> --alias wave1` |
| deps 自包含 | 命令里使用 `deps1` 时，同一命令行前面已有 `load_deps <deps.yaml> --alias deps1` |
| 链式分隔 | 多个 CLI 子命令之间使用 `--`，不要用多条 shell 命令分开承接 alias |
| 输出限流 | `find_*`、`read_hierarchy`、`list_signals`、`multi_signal_timeline` 等批量输出命令带 `--limit` |
| 时间窗口 | 非全局摘要类分析带 `--start/--end` 或 `--time-indices`，避免全波形扫描 |
| 证据有效 | 只有 exit code 为 0 且输出不含 alias/解析/环境错误的结果可写入报告 |
| Windows 调用 | PowerShell 用 `& "...\wave-analyzer-cli.exe"`；cmd 用 `"...\wave-analyzer-cli.exe"`；不要包含 `Shell` 字样或省略号 |

**失败输出处理**：
- `Waveform not found` / `unknown alias`：这是调用方式错误，不是设计问题；改成自包含链式命令重跑
- `Deps not found` / `dependency graph not loaded`：同一命令行补 `load_deps ... --alias deps1` 后重跑
- `parse error` / `invalid condition`：简化条件表达式，改用 `find_signal_events` + 直接读值验证
- `timeout` / 输出过大：缩小 `--start/--end`，增加 `--limit`，先找锚点再读取
- `文件名、目录名或卷标语法不正确`：这是 Windows shell 命令封装错误；去掉 `Shell` 前缀，补 PowerShell `&`，去掉 `…` 截断，重新执行完整命令
- `系统找不到指定的路径`：优先检查 exe/VCD/deps/RTL 路径是否存在、命令是否被 `…` 截断、PowerShell 是否缺少 `&`；不要解释为波形问题
- `Scope not found`：这是 VCD 层次路径错误，不是设计问题；必须先无 scope 读取层次，找到真实完整 scope 后重跑

**如果项目已有 deps.yaml（预生成），直接加载：**

```bat
<CLI> open_waveform <vcd_path> --alias wave1 ^
  -- load_deps deps.yaml --alias deps1 ^
  -- read_hierarchy wave1 --scope <top_module> --recursive true --limit 100
```

**如果 deps.yaml 不存在，先提取再加载：**

```bat
<CLI> extract_deps <rtl_path> <top_module> --output deps.yaml

<CLI> open_waveform <vcd_path> --alias wave1 ^
  -- load_deps deps.yaml --alias deps1 ^
  -- read_hierarchy wave1 --recursive true --limit 100
```

**信号路径映射**：
- deps.yaml 中的 `canonical` 路径使用 `TOP.<signal>` 格式
- 波形中的实际路径需要 `read_hierarchy` 确认
- 典型映射：`TOP.md_generate0.ge_for0[N]` ↔ VCD `gen_channel[N]`

### 层次发现协议

不要猜测 `u_dut`、`dut`、`tb.u_dut` 等 scope。首次分析必须按顺序发现真实层次：

1. **读取顶层候选**：

```powershell
& "<CLI>" open_waveform "<vcd_path>" --alias wave1 `
  -- read_hierarchy wave1 --recursive true --limit 200
```

2. **从输出中选择真实 scope**：只使用 `read_hierarchy` 输出里实际存在的完整路径，例如 `tb_top.u_dut`，不要只写末级名 `u_dut`
3. **深入子模块**：

```powershell
& "<CLI>" open_waveform "<vcd_path>" --alias wave1 `
  -- read_hierarchy wave1 --scope "<actual_scope_from_output>" --recursive true --limit 300
```

4. **确认信号完整路径**：测量前必须确认 clock/state/valid/data 信号出现在层次或 `list_signals` 输出中

如果 `Scope not found: <scope>`，回到第 1 步；不要继续用这个 scope 做测量。

### 并行分析前置条件

只有满足以下条件后才并行执行时钟测量、锚点定位、数据通路读取：

- 已经用无 scope 的 `read_hierarchy` 找到真实 top/scope
- 已经确认 clock、state、valid/done、关键数据通路信号的完整路径
- 每条并行 shell 命令都是自包含链式命令，都各自包含 `open_waveform`
- 每条并行命令都使用 PowerShell `& "<CLI>" ...` 或合法 cmd 形式

并行命令示例：

```powershell
& "<CLI>" open_waveform "<vcd_path>" --alias wave1 `
  -- measure_signal wave1 --signal "<actual_clk_path>" --analysis-type clock --edge-type posedge --limit 20
```

```powershell
& "<CLI>" open_waveform "<vcd_path>" --alias wave1 `
  -- find_signal_events wave1 "<actual_state_path>" --limit 30
```

### 步骤 2：定位时间锚点（并行）

**4 类锚点信号同时查询**：

以下为 CLI 子命令片段，执行时必须嵌入同一次 `open_waveform ... --alias wave1 -- ...` 链式调用，不能单独跨 shell 运行。

```text
# 时序锚点（tick/pulse 信号）
find_signal_events wave1 "<tick_signal>" --limit 20

# 状态锚点（FSM state）
find_signal_events wave1 "<state_signal>" --limit 30

# 输出锚点（valid/done 信号）
find_conditional_events wave1 "<valid_signal>" --limit 20

# 输入锚点（输入变化）
find_signal_events wave1 "<input_signal>" --limit 20
```

从时序锚点提取所有"脉冲高电平"时刻的 time_index 列表，作为后续批量读取的时间基准。

**⚠️ 必须用 `limit` 参数控制输出条数**，避免返回数百条无用事件。

### 稳定分析协议

为避免模型每轮选择不同路径导致结论漂移，固定按以下顺序分析：

1. **锚点定位**：只查时钟/tick、FSM state、valid/done、关键输入变化，得到候选 `time_index`
2. **窗口收敛**：围绕候选 `time_index` 选择小窗口，例如 `<idx-20, idx+80>`
3. **同窗测量**：用 `multi_signal_timeline` 或 `read_signal --time-indices` 在同一窗口读取所有关键信号
4. **源码对照**：按 RTL 中的状态转移、计数器、握手、移位逻辑逐项比对
5. **交叉验证**：BFS 只用于找依赖；最终判定必须来自直接测量
6. **结论落地**：每个 ✅/⚠️/❌ 后面必须列出命令、信号、time_index、实测值

读帧/应答类问题必须额外覆盖 `miso/rx_shift/rx_data/rx_valid/resp_valid/ack/done/timeout` 中实际存在的信号；未覆盖这些信号时，不允许给出"读功能正常"结论。

如果任何一步缺少有效输出，结论只能写"证据不足"，不能写"未发现问题"。

### 步骤 3：BFS 分析（需要 deps）

**如果不需要根因追踪，可跳过本步骤直接进入步骤 4。**

#### 3.1 建议入口信号

以下为 CLI 子命令片段，执行时必须嵌入同一次包含 `open_waveform` 和 `load_deps` 的链式调用。

```text
suggest_entry_signals wave1 deps1 --scope "<module_scope>" --limit 10 --simulator modelsim
```

选择 fan_in 较大、category 为 `dataOutput` 或 `controlOutput` 的信号。

#### 3.2 根因追踪

```text
trace_root_cause wave1 deps1 "<signal>" --time-index <idx> --max-depth 10 --simulator modelsim
```

#### 3.3 扇入分析

```text
find_fan_in deps1 "<canonical_signal_path>" --simulator modelsim
```

确认信号的直接驱动源。

#### 3.4 导出 BFS 报告

```text
export_bfs_report "<trace_id>" --format markdown
```

#### 3.5 兄弟信号扇入对比

当多个输出信号行为不一致时，同时查询所有兄弟信号的扇入锥对比差异。

#### 3.6 组合逻辑路径发现

`find_fan_in` 能自动识别 combinational (lat=0) vs sequential (lat=1) 路径，这是非 BFS 分析做不到的独特价值。

⛔ **但 `combinational, lat=0` 不等于"同拍生效"**（见规则 1）。

### 步骤 4：批量读取信号值 + 交叉验证

以下为 CLI 子命令片段，执行时必须嵌入同一次 `open_waveform ... --alias wave1 -- ...` 链式调用。

```text
# 多信号时序对齐
multi_signal_timeline wave1 ^
  --signals "<sig1>,<sig2>,<sig3>" ^
  --merge union --start <start> --end <end> ^
  --limit 50 --format hex

# 信号值提取
extract_signal_values wave1 --signal "<mosi>" --start <idx> --end <idx> --format hex

# 信号摘要
generate_summary wave1 --signal "<state>" --max-samples 30

# 信号比较
compare_signals wave1 --signals "<path1>,<path2>" --mode all_equal --start <idx> --end <idx>

# 条件事件查找
find_conditional_events wave1 "<condition>" --start <idx> --end <idx> --limit 20

# 序列检测
detect_sequence wave1 --steps "<cond1>,<cond2>" --max-gap 3000 --start <idx> --end <idx>

# 时间转换
time_convert wave1 --time-index <idx>
```

**逐项对照源码验证**，给出 ✅/❌ 判定。

### 步骤 5：信号特性测量

以下为 CLI 子命令片段，执行时必须嵌入同一次 `open_waveform ... --alias wave1 -- ...` 链式调用。

```text
# 时钟测量（频率、占空比、抖动）
measure_signal wave1 --signal "<clock>" --analysis-type clock --edge-type posedge --end <idx>

# 脉冲宽度测量
measure_signal wave1 --signal "<signal>" --analysis-type pulse --start <idx> --end <idx>
```

### 步骤 6：波形可视化（可选）

以下为 CLI 子命令片段，执行时必须嵌入同一次 `open_waveform ... --alias wave1 -- ...` 链式调用。

```text
export_svg wave1 --signal "<signal>" --time-range <start,end> --width 800 --height 200
```

### 步骤 7：生成分析报告

分析完成后，自动在 `reports/` 目录写入 Markdown 报告。

**报告结构**（严格按此顺序）：

```markdown
# 波形分析报告：<模块名>

## 1. 基本信息
（模块名、源码文件、波形文件、分析日期、设计模式、关键参数）

## 2. 设计概述
（状态机定义、关键参数、功能说明）

## 3. 验证结果总表
| # | 验证项 | 期望值 | 波形实测 | 判定 |
（✅ / ⚠️ / ❌）

## 4. 关键波形数据
### 4.1 时间锚点
### 4.2 采样点数据
### 4.3 输出数据

## 5. 发现的问题
（❌ 必须有交叉验证支撑）

## 6. 结论
（整体正确性判定 + 需修正问题总结）
```

**报告中的"波形实测"列必须填写从工具读取的具体数值**，不能只写"正确"。

## 工具命令速查

### BFS 核心（需 load_deps）

| 命令 | 用途 |
|------|------|
| `extract_deps <rtl> <top>` | 从 RTL 自动提取 deps.yaml |
| `load_deps <yaml>` | 加载依赖图 |
| `suggest_entry_signals <wave> <deps>` | 建议 BFS 入口信号 |
| `trace_root_cause <wave> <deps> <sig>` | 根因追踪 |
| `find_fan_in <deps> <sig>` | 扇入分析 |
| `export_bfs_report <trace_id>` | 导出 BFS 报告 |

### 波形基础（无需 deps）

| 命令 | 用途 |
|------|------|
| `open_waveform <path>` | 打开 VCD/FST |
| `read_hierarchy <wave>` | 读取层次结构 |
| `list_signals <wave>` | 列出信号 |
| `read_signal <wave> <sig>` | 读取信号值（支持 `--time-indices`） |
| `find_signal_events <wave> <sig>` | 信号变化事件 |
| `find_conditional_events <wave> <cond>` | 条件表达式事件 |
| `get_signal_info <wave> <sig>` | 信号信息 |
| `auto_discover_signals <wave>` | 自动发现信号 |
| `generate_summary <wave>` | 信号摘要 |
| `time_convert <wave>` | 时间索引转换 |

### 高级分析（无需 deps）

| 命令 | 用途 |
|------|------|
| `multi_signal_timeline <wave>` | 多信号时序对齐 |
| `measure_signal <wave>` | 时钟/脉冲/间隔测量 |
| `compare_signals <wave>` | 信号一致性验证 |
| `extract_signal_values <wave>` | 位流重构 |
| `analyze_handshake <wave>` | 握手协议分析 |
| `detect_sequence <wave>` | 序列检测 |
| `compute_crc <wave>` | CRC 校验 |
| `export_svg <wave>` | SVG 波形导出 |

### 辅助

| 命令 | 用途 |
|------|------|
| `check_env` | 环境诊断 |
| `help <cmd>` | 子命令帮助 |
| `analyze_run <summary.json>` | 自动化分析运行 |

## 工具限制与陷阱

### detect_sequence 条件表达式限制
复杂条件含 Verilog 字面量（如 `10'h001`）会解析失败。简单 `==` 条件可用。
替代：用 `find_signal_events` + `generate_summary`。

### measure_signal 参数格式
必须用 `--signal`（双横线），不能用 `-signal`（单横线）。

### find_signal_events 不支持 --format
直接调用即可，输出默认混合格式。

### auto_discover_signals 大文件超时
>100K 行 VCD 可能超时。替代：用 `read_hierarchy` + `list_signals --pattern`。

### find_conditional_events Verilog 字面量
含 `10'h020` 的条件会解析失败。替代：用 `find_signal_events`。

### compare_signals 异步 FIFO
din/dout 属不同时钟域，"Mismatch" 是正常的异步 FIFO 行为。
正确做法：用 `multi_signal_timeline` 对齐 wr_en/rd_en/din/dout。

### analyze_handshake 不适用 FIFO 写侧
wr_en/empty 不构成标准握手。替代：手动时序分析。

### multi_signal_timeline wire 信号限制
某些内部 wire 在特定层级不可见。替代：在更深子模块层级查找。

## 效率优化

1. **链式调用**：CLI 无状态，必须在同一次调用中用 `--` 连接所有命令；每条使用 `wave1/deps1` 的 shell 命令都必须自行包含 `open_waveform/load_deps`
2. **并行查询**：多个独立分析可用当前环境的 shell/terminal 工具并行执行
3. **时间锚点驱动**：先定位事件，再批量读取，避免全量扫描
4. **合理 limit**：控制输出条数，避免返回数百条无用数据
5. **read_signal --time-indices**：一次读取多个关键时刻，效率远高于逐时刻查询

## 参考文档

详细的设计模式锚点对照表、验证清单模板、CLI 使用最佳实践见 [reference.md](reference.md)。

## 可复制到模型提示词的最小协议

```text
使用 wave-analyzer-cli.exe 时必须遵守：
1. CLI 是无状态进程。不得跨 shell 命令复用 wave1/deps1。
2. 任何使用 wave1 的命令，同一命令行必须先 open_waveform <vcd> --alias wave1。
3. 任何使用 deps1 的命令，同一命令行必须先 load_deps <deps.yaml> --alias deps1。
4. 多个 CLI 子命令必须用 -- 链接。
5. 批量查询必须带 --limit；局部分析必须带 --start/--end 或 --time-indices。
6. Waveform not found / unknown alias / deps not found 是调用失败，不是设计结论，必须重写链式命令重跑。
7. 不允许只凭 BFS 下结论；每个问题结论必须有直接信号测量值、time_index、命令输出支撑。
8. 证据不足时只能说证据不足，不能说未发现问题。
9. Windows PowerShell 调用 exe 必须写成 & "完整exe路径" 参数...；不要把 Shell 标签写入命令，不要使用 --ou… 这类省略号截断参数。
10. 不得猜测 scope。必须先 read_hierarchy wave1 --recursive true --limit 200 找到真实完整 scope，再深入子模块或测量信号。
11. Scope not found 是层次路径错误；系统找不到指定路径/文件名目录名语法不正确是 shell/路径错误；都不能写入设计分析结论。
12. 并行分析前必须已确认真实 scope 和完整信号路径；每条并行命令都必须独立 open_waveform。
13. 遇到 read/rd/resp/ack/MISO/rx_valid/MMIC/应答/读帧相关任务，必须验证读请求后的 MISO/rx_valid/rx_data/ack/done/timeout；未检查响应链不能说未发现问题。
```
