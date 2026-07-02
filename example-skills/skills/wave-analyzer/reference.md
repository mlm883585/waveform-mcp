# wave-analyzer 参考文档 — 设计模式锚点与 CLI 最佳实践

---

## 一、设计模式识别

通过阅读 Verilog 源码，根据以下特征识别设计模式：

| 设计模式 | 关键特征信号 | 源码识别线索 |
|---|---|---|
| **UART TX/RX** | `baud_tick`, `rx_sync`, `tx_shift`, `start/stop位` | 参数含 `BAUD_RATE`; 状态含 `START/DATA/STOP`; 有同步链 `rx_sync1/2/3` |
| **SPI 主/从** | `spi_clk`, `cs_n`, `mosi/miso`, `shift` | 参数含 `SPI_MODE/CPOL/CPHA`; 有时钟生成逻辑; 有 `cs_n` 控制 |
| **AXI4** | `awvalid/awready`, `wvalid/wready`, `bvalid/bready` | 状态含 `AW/W/B/AR/R` 通道握手 |
| **通用 FSM** | `state`, `next_state`, `valid/done` | 有 `localparam` 状态编码; 三段式状态机 |
| **计数器/定时器** | `tick`, `overflow`, `enable`, `count` | 有分频计数逻辑; 周期性 tick 输出 |
| **PWM** | `duty`, `period_cnt`, `pwm_out` | 有占空比参数; 周期计数和比较 |
| **FIFO** | `wr_en/rd_en`, `full/empty`, `wr_ptr/rd_ptr` | 有读写指针; 满/空标志; 双端口RAM |
| **跨时钟域** | `sync_ff1/2/3`, `toggle`, `stretch_cnt` | 有 2-3 级同步链; 翻转+边沿检测; 脉冲展宽 |

---

## 二、各模式锚点信号与验证清单

### UART 接收 (uart_rx)

**锚点信号**：

| 锚点类别 | 信号 | 查询方式 |
|---|---|---|
| 时序锚点 | `dut.baud_tick` | `find_signal_events` |
| 状态锚点 | `dut.state` | `find_signal_events` |
| 输出锚点 | `dut.rx_valid` | `find_conditional_events` |
| 输入锚点 | `rx_in` | `find_signal_events` |

**验证清单**：

| # | 验证项 | 期望值 | 读取信号 |
|---|---|---|---|
| 1 | 同步链延迟 | 每级延迟1时钟 | `rx_sync1/2/3` @ 输入跳变前后 |
| 2 | 下降沿检测 | `sync3 & ~sync2` | `rx_fall` @ 同步链验证时刻 |
| 3 | 起始位确认 | 半波特后 sync2=0 | `rx_sync2` @ START tick |
| 4 | 状态转换 | IDLE→START→DATA→STOP→IDLE | `state` 事件列表 |
| 5 | 波特率时序 | 间距 = BAUD_DIV × CLK_PERIOD | tick 事件时间差 |
| 6 | 数据位采样 | sync2 与输入位一致 | `rx_sync2` @ DATA ticks |
| 7 | LSB-first 移位 | `{sync2, shift[7:1]}` | `rx_shift` @ 所有 ticks |
| 8 | 最终数据输出 | = 正确字节 | `rx_data` @ valid 时刻 |
| 9 | valid 脉冲宽度 | 单时钟周期 | `rx_valid` @ valid 前后 |
| 10 | bit_cnt | 0→1→...→7→0 | `bit_cnt` @ 所有 ticks |

### UART 发送 (uart_tx)

**验证清单**：

| # | 验证项 | 期望值 |
|---|---|---|
| 1 | 状态转换 | IDLE→START→DATA→STOP→DONE→IDLE |
| 2 | 波特率时序 | 间距 = BAUD_DIV × CLK_PERIOD |
| 3 | 起始位 | tx_out=0 持续1波特周期 |
| 4 | LSB-first 发送 | tx_out 低位先发 |
| 5 | 停止位 | tx_out=1 持续1波特周期 |
| 6 | tx_done | 单周期脉冲 |
| 7 | busy 标志 | 发送期间=1, IDLE=0 |

### SPI 主/从 (spi_master / spi_slave)

**锚点信号**：`spi_clk` 边沿, `cs_n` 跳变, `tx_valid/rx_valid`

**验证清单**：

| # | 验证项 | 期望值 |
|---|---|---|
| 1 | cs_n 时序 | 传输前拉低，传输后拉高 |
| 2 | spi_clk 频率 | = 预设分频值 |
| 3 | CPOL/CPHA | 极性和相位符合配置 |
| 4 | 数据移位方向 | MSB 或 LSB first |
| 5 | 发送数据 | mosi 与 tx_data 对应 |
| 6 | 接收数据 | rx_data 与 miso 对应 |
| 7 | 传输完成 | 单周期 valid/done 脉冲 |
| 8 | MOSI 移位寄存器 | 字节从 LSB 端插入，左移 8 位 |
| 9 | MISO 捕获 | 按 bit_cnt 从 MSB 到 LSB |
| 10 | CSN 低脉冲宽度 | = total_bits × T_spi_clk + hold |

### SPI/MMIC 读帧应答专项

当 SPI 事务包含读命令、读寄存器、read frame、MMIC response、应答帧、返回数据时，必须额外验证"请求 → 应答"因果链。只验证 `cs_n/spi_clk/mosi` 正常不能证明读帧成功。

**必查信号类别**：

| 类别 | 典型信号名 |
|---|---|
| 读请求触发 | `read_req`, `rd_en`, `cmd_valid`, `tx_valid`, `start`, `r_read_state` |
| 片选/时钟 | `cs_n`, `spi_clk`, `sclk`, `bit_cnt`, `word_cnt` |
| 发送命令 | `mosi`, `tx_data`, `tx_shift`, `cmd_byte`, `addr` |
| 返回应答 | `miso`, `rx_data`, `rx_shift`, `rx_valid`, `resp_valid`, `ack`, `done` |
| 超时/错误 | `timeout`, `err`, `no_resp`, `busy`, `state` |

**验证清单**：

| # | 验证项 | 期望值 | 判定依据 |
|---|---|---|---|
| 1 | 读命令发出 | MOSI/tx_data 出现读 opcode + 地址 | 读请求窗口内的 `multi_signal_timeline` |
| 2 | 读帧时钟数量 | 读命令后继续产生足够 SCLK 采样返回位 | `bit_cnt/word_cnt/spi_clk` |
| 3 | 从设备返回 | MISO 有有效变化或符合协议的固定响应值 | `miso/rx_shift` 直接测量 |
| 4 | 接收有效 | `rx_valid/resp_valid/ack/done` 在期望窗口内拉高 | 条件事件或同窗时序 |
| 5 | 接收数据落地 | `rx_data/resp_data` 与 MISO 位流一致 | `extract_signal_values` + `read_signal` |
| 6 | 无应答识别 | 读命令已发出但响应窗口内无 `rx_valid/ack`，或 `timeout/no_resp` 置位 | 请求窗口与响应窗口交叉验证 |

**报告要求**：
- 如果读请求存在，但响应窗口内没有 `rx_valid/resp_valid/ack/done`，必须报告"读帧无应答"或"证据显示无有效响应"
- 如果没有检查 MISO/rx_valid/rx_data/ack/done，不能写"读功能正常"或"未发现问题"
- 如果 testbench 没有驱动 MISO 或外设响应模型，结论应为"仿真环境未提供 MMIC 应答"，不是 RTL 必然错误
- 必须区分"请求未发出"、"请求发出但无时钟"、"有时钟但 MISO 无响应"、"MISO 有响应但 rx_valid 未拉高"

### AXI4 通道

**锚点信号**：`awvalid/awready`, `wvalid/wready`, `bvalid/bready`, 数据通道

**验证清单**：

| # | 验证项 | 期望值 |
|---|---|---|
| 1 | AW 握手 | awvalid↑ → awready↑ → awaddr 稳定 |
| 2 | W 握手 | wvalid↑ → wready↑ → wdata 稳定 |
| 3 | B 响应 | bvalid↑ → bready↑ → bresp=OK |
| 4 | AR 握手 | arvalid↑ → arready↑ → araddr 稳定 |
| 5 | R 数据 | rvalid↑ → rready↑ → rdata 正确 |

### 跨时钟域同步 (CDC)

**锚点信号**：`sync_ff1/2/3`, `toggle`, `stretch_cnt`, `cmd_sof`

**验证清单**：

| # | 验证项 | 期望值 |
|---|---|---|
| 1 | 同步链延迟 | ff1→ff2→ff3 逐级延迟 1 个目标时钟周期 |
| 2 | 边沿检测 | ff2 ^ ff3 产生单周期脉冲 |
| 3 | 脉冲展宽 | stretch_cnt 加载最大值后逐拍递减 |
| 4 | CDC 总延迟 | ≤ 3 个目标时钟周期 |
| 5 | 展宽覆盖 | stretch_cnt > 0 期间覆盖整个 FIFO 读取 |
| 6 | SOF 三保险 | 边沿脉冲 || 展宽保持 || FIFO 非空 |

### 多通道 Generate (multi-channel)

**锚点信号**：`w_cmd_wen`, `w_cmd_sof`, `channel[N].fifo.empty/rd_en/dout`

**验证清单**：

| # | 验证项 | 期望值 |
|---|---|---|
| 1 | 路由正确性 | wen bit[N]=1 对应通道 N |
| 2 | 帧格式解析 | 短帧/BEC/全通道帧各自正确路由 |
| 3 | FIFO 写入字节数 | 短帧 4B, BEC 4B, 全通道 14B |
| 4 | FIFO 读出字节数 | 与写入一致 |
| 5 | 通道独立性 | 不同通道 FIFO 互不干扰 |
| 6 | 优先级 | 短帧 > BEC > 全通道帧 |

---

## 三、CLI 使用最佳实践

### 链式调用（必须）

CLI 无状态，每次调用都是独立进程。所有命令必须在同一次调用中用 `--` 连接：

```bat
<CLI> open_waveform file.vcd --alias w1 ^
  -- load_deps deps.yaml --alias d1 ^
  -- find_signal_events w1 "dut.state" --limit 30 ^
  -- multi_signal_timeline w1 --signals "dut.a,dut.b" --start 0 --end 1000 --format hex
```

### read_signal --time-indices 批量读取

最高效的信号读取方式。一次读取多个时间戳：

```bash
read_signal wave1 "dut.rx_sync2" --time-indices 659,1527,2395,3263,4131,4999
```

### multi_signal_timeline — 多信号时序对齐

SPI/FSM 传输验证的核心工具：

```bat
multi_signal_timeline wave1 ^
  --signals "dut.state,dut.csn,dut.bit_cnt,dut.mosi" ^
  --merge union --start 100 --end 3000 ^
  --limit 50 --format hex
```

- `--merge union`：所有信号转换点取并集
- `--format hex|binary|decimal`：值格式
- 输出为对齐表格，一行一个时间点

### measure_signal — 精确测量

```bat
# 时钟测量
measure_signal wave1 --signal "dut.clk" --analysis-type clock --edge-type posedge

# 脉冲宽度
measure_signal wave1 --signal "dut.csn" --analysis-type pulse --start 50 --end 3000

# 时间间隔（条件触发）
measure_signal wave1 --signal "dut.clk" --analysis-type interval ^
  --from-condition "dut.state==2" --to-condition "dut.done==1" ^
  --expected-value 1000 --expected-unit ns
```

### compare_signals — 一致性验证

```bat
compare_signals wave1 ^
  --signals "engine.mosi,channel.mosi" ^
  --mode all_equal --start 100 --end 3000
```

⚠️ 异步 FIFO din/dout "Mismatch" 是正常的（跨时钟域延迟）。

### extract_signal_values — 位流重构

```bat
extract_signal_values wave1 --signal "dut.mosi" ^
  --start 100 --end 2000 --format binary
```

从 1-bit 信号的所有转换点提取值，可手动重建 SPI 传输字节。

### detect_sequence — 序列检测

```bat
# 简单条件可用
detect_sequence wave1 ^
  --steps "dut.csn==0,dut.csn==1" ^
  --max-gap 3000 --start 0 --end 3600 --limit 10
```

⚠️ 含 Verilog 字面量（如 `10'h001`）的条件会解析失败，用 `find_signal_events` 替代。

### generate_summary — 快速概览

```bash
generate_summary wave1 --signal "dut.state" --max-samples 50
```

输出 JSON 格式的信号采样摘要，快速了解 FSM 状态转换序列。

### time_convert — 时间转换

```bash
time_convert wave1 --time-index 567
```

### export_svg — 波形可视化

```bash
export_svg wave1 --signal "dut.csn" --time-range 100,500 --width 800 --height 200
```

---

## 四、BFS 深度用法

### 信号路径映射

deps.yaml 中的 canonical 路径使用 `TOP.<signal>` 格式，VCD 中的实际路径可能不同：

| deps canonical | VCD 实际路径 | 说明 |
|---|---|---|
| `TOP.clk` | `tb_top.u_dut.clk` | 顶层信号 |
| `TOP.md_generate0.ge_for0[N].gen_channel.u_channel.<sig>` | `tb_top.u_dut.gen_channel[N].u_channel.<sig>` | generate 实例 |
| `TOP.u_demux.<sig>` | `tb_top.u_dut.u_demux.<sig>` | 子模块 |

⚠️ generate 索引可能有偏移，需结合 `read_hierarchy` 验证。

### trace_root_cause 多信号链式

可在同一次链式调用中对多个信号执行 trace：

```bat
<CLI> open_waveform file.vcd --alias w1 ^
  -- load_deps deps.yaml --alias d1 ^
  -- trace_root_cause w1 d1 "dut.r_active" --time-index 435 --max-depth 10 --simulator modelsim ^
  -- trace_root_cause w1 d1 "dut.r_fifo_cnt" --time-index 435 --max-depth 10 --simulator modelsim
```

⚠️ 各信号的值来自不同时间点（规则 2），不能拼成同一拍快照。

### find_fan_in combinational vs sequential

```bash
find_fan_in d1 "TOP.u_demux.r_bec_fifo_cnt" --simulator modelsim
```

输出示例：
```
r_bec_active → combinational, lat=0    (next-state 使能条件)
rst → control                           (复位)
r_bec_fifo_cnt → sequential, lat=1      (自反馈)
clk → sequential, lat=1                 (时钟)
```

⛔ `combinational, lat=0` 只说明 `r_bec_active` 是 `r_bec_fifo_cnt` 的 next-state 组合输入。
NBA 语义下：active 首次拉高当拍，fifo_cnt 仍然是 0，下一拍才变成 1。

### Cyclic 节点

`trace_root_cause` 中的 `Cyclic` 节点表示自反馈（如计数器递增），这是正常的寄存器行为。

---

## 五、从源码快速提取关键信息

```bash
# 状态机定义
grep -n "localparam.*STATE\|localparam.*S_" rtl/*.v

# 分频参数
grep -n "BAUD_DIV\|SPI_DIV\|CLK_DIV\|PRESCALER\|PERIOD" rtl/*.v

# 同步链
grep -n "sync_ff\|_s1\|_s2\|_s3\|r_rst.*sync" rtl/*.v

# FIFO 信号
grep -n "wr_en\|rd_en\|din\|dout\|empty\|full" rtl/*.v

# 握手信号
grep -n "valid\|ready\|_en\|_ack\|done\|busy" rtl/*.v
```

---

## 六、典型分析效率

| 设计复杂度 | 信号数 | 锚点数 | 预估调用 | 预估轮次 |
|---|---|---|---|---|
| 简单（UART 单通道） | ~15 | ~10 | ~15 | 3 |
| 中等（SPI 双通道） | ~30 | ~20 | ~20 | 3 |
| 复杂（多通道+CDC） | ~50+ | ~30+ | ~25 | 3 |

无论复杂度如何，轮次始终为 3——只有每轮内的并行调用数量增加。
