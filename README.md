# Wave 波形分析助手

[![Crates.io Version](https://img.shields.io/crates/v/wave-analyzer-mcp)](https://crates.io/crates/wave-analyzer-mcp)

An MCP (Model Context Protocol) server for reading and analyzing waveform files (VCD/FST format) using the [wellen](https://github.com/ekiwi/wellen) library.

## Usage

### Installation

Install via cargo:

```bash
cargo install wave-analyzer-mcp
```

The built binary will be at `~/.cargo/bin/wave-analyzer-mcp`.

Install manually:

```bash
# Clone the repository
git clone https://github.com/jiegec/wave-analyzer-mcp
cd waveform-mcp

# Build the server
cargo build --release
```

The built binary will be at `target/release/wave-analyzer-mcp`.

### Running

```bash
# Run the server with stdio transport (default)
target/release/wave-analyzer-mcp

# Run the server in HTTP mode
target/release/wave-analyzer-mcp --http

# Run the server in HTTP mode with custom bind address
target/release/wave-analyzer-mcp --http --bind-address 0.0.0.0:8000
```

The server supports two transport modes:

- **Stdio mode** (default): Uses standard input/output for MCP communication
- **HTTP mode**: Uses streamable HTTP server for remote access at `/mcp` endpoint

When running in HTTP mode, the server listens on the specified bind address (default: `127.0.0.1:8000`). HTTP mode allows the waveform store to be shared across multiple HTTP sessions, enabling remote analysis of waveform files.

## Features

- Open VCD (Value Change Dump) and FST (Fast Signal Trace) waveform files
- List all signals in a waveform with hierarchical paths
- Read the waveform module hierarchy as an indented tree
- Read signal values at specific time indices (single or multiple)
- Get signal metadata (type, width, index range)
- Find signal events (changes) within a time range
- Find conditional events with Verilog-style expression parser (LALRPOP)
- Format time values with timescale information (e.g., "10ns", "5000ps")
- Load dependency graphs (deps.yaml) with fan-in/fan-out indices and signal/clock aliases
- Parse ModelSim assertion logs (4 transcript formats)
- Load design specs (design_spec.yaml) mapping assertions to BFS entry signals
- BFS root-cause tracing with clock-edge time backtrack, cycle detection, and candidate ranking
- Suggest BFS entry signals with 3-tier priority ranking
- Extract signal values with multi-bit reconstruction from individual bit signals
- Analyze valid/ready handshake protocols with latency statistics
- Measure clock characteristics (period, frequency, duty cycle, jitter) and pulse widths
- Compare multiple signals and detect mismatches (with bit_mapping reconstruction)
- Build logic-analyzer-style multi-signal timelines
- Auto-discover bus slices, clocks, and reset signals in waveform hierarchy
- Detect ordered condition sequences with max gap constraint
- Compute CRC (CRC-8, CRC-16-CCITT, CRC-32-Ethernet) and verify against observed CRC
- Streamable HTTP server support for remote access

## Tools

The server provides 27 MCP tools:

### Basic Waveform Tools (8)

1. **open_waveform** - Open a waveform file
   - `file_path`: Path to .vcd or .fst file
   - `alias`: Optional alias for the waveform (defaults to filename)

   **Example response:**
   ```
   Waveform opened successfully with alias: waveform.vcd
   ```

2. **close_waveform** - Close a waveform and free its memory
   - `waveform_id`: ID or alias of the waveform to close

   **Example response:**
   ```
   Waveform 'waveform.vcd' closed successfully
   ```

3. **list_signals** - List all signals in an open waveform
   - `waveform_id`: ID or alias of the waveform
   - `name_pattern`: Optional substring to filter signals by name (case-insensitive)
   - `hierarchy_prefix`: Optional prefix to filter signals by hierarchy path
   - `recursive`: Optional flag to include signals from sub-hierarchies (default: true)
   - `limit`: Optional maximum number of signals to return (default: 100)

   **Example response:**
   ```
   Found 3 signals:
   top.clock
   top.reset
   top.data
   ```

4. **read_hierarchy** - Read the waveform module hierarchy as an indented tree
   - `waveform_id`: ID or alias of the waveform
   - `scope_path`: Optional scope path to start from
   - `recursive`: Optional flag to include descendants (default: false)
   - `limit`: Optional maximum number of modules to return (default: 200)

   **Example response:**
   ```
   Hierarchy rooted at 'top':
   top
     submodule
   ```

5. **read_signal** - Read signal values at specific time indices
   - `waveform_id`: ID or alias of the waveform
   - `signal_path`: Hierarchical path to signal (e.g., "top.module.signal")
   - `time_index`: Optional single time index to read
   - `time_indices`: Optional array of time indices to read multiple values

   **Example response:**
   ```
   Time index 0 (0ns): 0
   Time index 10 (10ns): 1
   Time index 20 (20ns): 1
   ```

6. **get_signal_info** - Get metadata about a signal
   - `waveform_id`: ID or alias of the waveform
   - `signal_path`: Hierarchical path to signal

   **Example response:**
   ```
   Signal: top.data
   Type: Wire
   Width: 8 bits
   Index: [7:0]
   ```

7. **find_signal_events** - Find all signal changes within a time range
   - `waveform_id`: ID or alias of the waveform
   - `signal_path`: Hierarchical path to signal
   - `start_time_index`: Optional start of time range (default: 0)
   - `end_time_index`: Optional end of time range (default: last time index)
   - `limit`: Optional maximum number of events to return (default: 100)

   **Example response:**
   ```
   Found 3 events for signal 'top.clock' (time range: 0 to 20):
   Time index 0 (0ns): 0
   Time index 10 (10ns): 1
   Time index 20 (20ns): 0
   ```

8. **find_conditional_events** - Find events where a condition is satisfied
   - `waveform_id`: ID or alias of waveform
   - `condition`: Conditional expression to evaluate
   - `start_time_index`: Optional start of time range (default: 0)
   - `end_time_index`: Optional end of time range (default: last time index)
   - `limit`: Optional maximum number of events to return (default: 100)

   **Example response:**
   ```
   Found 2 events for condition '!$past(TOP.signal) && TOP.signal' (time range: 0 to 50):
   Time index 5 (50ns): top.signal = 8'h0A
   Time index 15 (150ns): top.signal = 8'hFF
   ```

   **Supported condition syntax:**
   - Signal paths (e.g., `TOP.signal`)
   - Bitwise operators: `~` (NOT), `&` (AND), `|` (OR), `^` (XOR)
   - Boolean operators: `&&` (AND), `||` (OR), `!` (NOT)
   - Comparison operators: `==`, `!=`
   - Parentheses for grouping: `(condition)`
   - `$past(signal)` - read signal value from previous time index
   - Verilog-style literals: `4'b0101` (binary), `3'd2` (decimal), `5'h1A` (hex)
   - Bit extraction: `signal[bit]` for single bit, `signal[msb:lsb]` for range

   **Operator precedence (highest to lowest):**
   1. `~`, `!` (bitwise NOT, logical NOT)
   2. `==`, `!=` (equality/inequality)
   3. `&` (bitwise AND)
   4. `^` (bitwise XOR)
   5. `|` (bitwise OR)
   6. `&&` (logical AND)
   7. `||` (logical OR)

### Advanced Analysis Tools (8)

9. **load_dependencies** - Load a deps.yaml dependency graph
   - `file_path`: Path to deps.yaml file
   - `alias`: Optional alias (defaults to filename)

   **Example response:**
   ```
   Dependency graph loaded with alias: deps.yaml
   Version: 1.0
   Nodes: 15, Edges: 23
   Signal aliases: 5, Clock aliases: 2
   Has cycles: true
   ```

10. **load_assertion_log** - Parse ModelSim transcript for assertion events
    - `file_path`: Path to transcript.log
    - `alias`: Optional alias (defaults to filename)
    - `severity_filter`: Optional list of severities to include (Error, Warning, Note, Failure)
    - `limit`: Optional max events to return (-1 = unlimited)

    **Example response:**
    ```
    Assertion log loaded with alias: transcript.log
    Parsed events: 3, Unmatched lines: 0
    Top events:
    - Error ASSERT_PASSED @ 1750 ns in TOP.ch0
    - Warning SETUP_CHECK @ 2000 ns in TOP.ctrl
    ```

11. **load_design_spec** - Load a design_spec.yaml file (OPTIONAL)
    - `file_path`: Path to design_spec.yaml
    - `alias`: Optional alias (defaults to filename)

    Maps assertions to entry signals, provides debug hints and stop signals.
    If you don't have a design_spec.yaml, use `suggest_entry_signals` instead.

    **Example response:**
    ```
    Design spec loaded with alias: design_spec.yaml
    Debug hints available: true
    Debug entry points: 3, Stop signals: 2
    ```

12. **trace_root_cause** - BFS root-cause tracing from a failing signal
    - `waveform_id`: ID of open waveform
    - `deps_id`: ID of loaded dependency graph
    - `signal_path`: Entry signal path (use `suggest_entry_signals` if unknown)
    - `time_index`: OR use `time_value` + `time_unit` (e.g., 30, "ns")
    - `spec_id`: Optional design spec alias for debug hints
    - `max_depth`: Optional max BFS depth (default: 8)
    - `simulator`: Optional simulator name for alias resolution (default: "modelsim")

    **Example response:**
    ```
    BFS trace complete. 12 nodes explored.
    Root cause candidates:
    1. TOP.ch0.coeff_valid @ 1750ns (Suspect)
    2. TOP.ctrl.output_enable @ 1750ns (Boundary)
    ```

13. **find_fan_in** - Query upstream dependency edges for a signal
    - `deps_id`: ID of loaded dependency graph
    - `signal_path`: Signal to query
    - `simulator`: Optional simulator name for alias resolution (default: "modelsim")

    **Example response:**
    ```
    Found 3 fan-in edges for 'TOP.output':
    - TOP.input_a -> TOP.input_a [sequential] clock=clk_sys latency=1
    - TOP.enable -> TOP.enable [control]
    - TOP.sel -> TOP.sel [combinational]
    ```

14. **find_fan_out** - Query downstream dependent signals for a signal
    - `deps_id`: ID of loaded dependency graph
    - `signal_path`: Signal to query
    - `simulator`: Optional simulator name for alias resolution (default: "modelsim")

    **Example response:**
    ```
    Found 2 fan-out signals for 'TOP.input_a':
    - TOP.input_a -> TOP.output [sequential] latency=1
    - TOP.input_a -> TOP.fifo_data [data]
    ```

15. **batch_trace_root_cause** - Batch BFS root-cause tracing for all assertion events
    - `waveform_id`: ID of open waveform
    - `deps_id`: ID of loaded dependency graph
    - `assertion_id`: ID of loaded assertion log (from load_assertion_log)
    - `spec_id`: Optional design spec ID for entry signal resolution
    - `max_depth`: Optional max BFS depth per trace (default: 8)
    - `severity_filter`: Optional severity filter (e.g., "Error,Failure")
    - `simulator`: Optional simulator name for alias resolution (default: "modelsim")

    **Example response:**
    ```
    Batch trace complete. 3 events traced, 5 root cause candidates aggregated.
    Top candidates: TOP.ctrl.enable (2 occurrences), TOP.ch0.coeff_valid (1 occurrence)
    ```

16. **export_bfs_report** - Export a BFS trace result as a formatted report
    - `trace_id`: Trace ID returned by trace_root_cause or batch_trace_root_cause
    - `format`: Output format: "json", "markdown", or "html" (default: "markdown")

    **Example response:**
    ```
    # BFS Root Cause Report
    Signal: TOP.data_o, Time: 1750ns
    Candidates:
    | Signal | Status | Reason |
    | TOP.ctrl.enable | Suspect | Value changed at 1750ns |
    | TOP.ch0.coeff_valid | Boundary | Clock edge trigger |
    ```

17. **suggest_entry_signals** - Recommend BFS entry signals when no design_spec.yaml
    - `waveform_id`: ID of open waveform
    - `deps_id`: ID of loaded dependency graph
    - `assertion_name`: Optional assertion name for matching
    - `scope_path`: Optional scope to limit search
    - `limit`: Optional max suggestions (default: 10)
    - `simulator`: Optional simulator name (default: "modelsim")

    **Example response:**
    ```
    Suggested entry signals:
    Tier 1 (deps output): TOP.output, TOP.valid
    Tier 2 (boundary): TOP.input_a, TOP.clk_sys
    Tier 3 (other): TOP.debug_signal
    ```

### Signal Value Extraction & Protocol Analysis Tools (3)

18. **extract_signal_values** - Extract signal values or reconstruct multi-bit signals
    - `waveform_id`: ID of open waveform
    - `signal_path`: Optional single signal path to extract
    - `bit_mapping`: Optional list of (bit_position, signal_path) for multi-bit reconstruction
    - `start_time_index` / `end_time_index`: Optional time range
    - `value_format`: "hex" (default), "binary", or "decimal"
    - `downsample`: Optional max sample points

    **Example response:**
    ```
    Signal: reconstructed[15:0], Width: 16
    Total changes: 42, Samples returned: 42
    16'h0A3F (0ns), 16'h0001 (10ns), 16'hFFFF (50ns)
    ```

19. **analyze_handshake** - Analyze valid/ready handshake protocol
    - `waveform_id`: ID of open waveform
    - `valid_signal`: Path to valid signal
    - `ready_signal`: Path to ready signal
    - `data_signal`: Optional path to data signal for value capture
    - `start_time_index` / `end_time_index`: Optional time range
    - `report_mode`: "summary" (default) or "detailed"
    - `limit`: Optional max transfers to report

    **Example response:**
    ```
    Handshake analysis: 15 transfers, 3 stalls
    Average latency: 2 cycles, Min: 1, Max: 5
    ```

20. **measure_signal** - Measure clock or pulse signal characteristics
    - `waveform_id`: ID of open waveform
    - `signal_path`: Path to the signal to measure
    - `analysis_type`: "clock" or "pulse"
    - For clock: `edge_type` (posedge/negedge/both)
    - For pulse: measures high/low pulse width statistics
    - `start_time_index` / `end_time_index`: Optional time range

    **Example response (clock):**
    ```
    Clock analysis: period=10ns, frequency=100MHz, duty_cycle=50%, jitter=0.2ns
    ```

### Generic Algorithm Tools (5)

21. **compare_signals** - Compare two or more signals and find mismatches
    - `waveform_id`: ID of open waveform
    - `signals`: List of signal references (path or bit_mapping)
    - `comparison_mode`: "all_equal" (default) or "reference_vs_actual"
    - `start_time_index` / `end_time_index`: Optional time range
    - `value_format`: "hex" (default), "binary", or "decimal"
    - `limit`: Optional max mismatches to report

    **Example response:**
    ```
    Signal Comparison Report
    Signals: TOP.expected vs TOP.actual
    Total comparisons: 50, Mismatches: 3
    Mismatch #1 at time 10ns: Expected=8'h5A, actual=8'h00
    ```

22. **multi_signal_timeline** - Build a logic-analyzer-style unified timeline
    - `waveform_id`: ID of open waveform
    - `signals`: List of signal paths to include
    - `merge_mode`: "union" (default) or "intersection"
    - `value_format`: "hex" (default), "binary", or "decimal"
    - `start_time_index` / `end_time_index`: Optional time range
    - `limit`: Optional max time points

    **Example response:**
    ```
    Timeline: 20 time points, 3 signals
    0ns: clk=0, data=8'h00, valid=0
    5ns: clk=1, data=8'h5A, valid=1
    ```

23. **auto_discover_signals** - Auto-discover signal patterns in waveform
    - `waveform_id`: ID of open waveform
    - `discovery_mode`: "bus_slices" (default), "clocks", "groups", or "all"
    - `scope_path`: Optional scope to limit search
    - `pattern`: Optional regex filter on signal names

    **Example response:**
    ```
    Bus slices found: crc[0]..crc[15] (16-bit bus), data_0..data_7 (8-bit bus)
    Clocks detected: TOP.clk (100MHz regular), TOP.clk_fast (200MHz regular)
    ```

24. **detect_sequence** - Detect ordered condition sequences with max gap constraint
    - `waveform_id`: ID of open waveform
    - `sequence`: Ordered list of condition expressions
    - `max_gap_cycles`: Maximum allowed gap between consecutive steps
    - `start_time_index` / `end_time_index`: Optional time range
    - `limit`: Optional max sequences to report

    **Example response:**
    ```
    Sequence detected: 3 occurrences
    #1: step1@5ns → step2@7ns → step3@10ns (gap: 2 cycles)
    ```

25. **compute_crc** - Compute CRC over a data signal and optionally verify
    - `waveform_id`: ID of open waveform
    - `data_signal_path`: Path to data bus signal
    - `crc_signal_path`: Optional path to observed CRC for verification
    - `crc_polynomial`: "crc-8", "crc-16-ccitt", or "crc-32-ethernet"
    - `initial_value`: Optional CRC initial value
    - `start_time_index` / `end_time_index`: Optional time range
    - `limit`: Optional max data points

    **Example response:**
    ```
    CRC computation: 50 data points, polynomial=CRC-32-Ethernet
    Verification: 5 mismatches found against observed CRC signal
    ```

### Waveform Summary Tools (2)

26. **get_waveform_summary** - Generate a summary of waveform statistics and sampled values
    - `file_path`: Path to the waveform file (.vcd or .fst)
    - `signals`: List of signal paths to include (empty = auto-detect top signals)
    - `max_samples`: Optional max samples per signal (default: 100)

    **Example response:**
    ```
    Waveform summary:
    File: dump.vcd, Duration: 0ns to 5000ns
    Total signals sampled: 5, Samples per signal: 100
    Signal: top.clk (width=1, type=clock, 100 samples)
    Signal: top.data (width=8, type=wire, 100 samples)
    ```

27. **export_waveform_svg** - Export waveform visualization to SVG
    - `waveform_id`: ID of open waveform (from open_waveform)
    - `signals`: List of signal paths to include
    - `time_range`: Optional (start, end) tuple in time indices
    - `width`: Optional output width in pixels (default: 800)
    - `height`: Optional output height in pixels (default: 600)

    **Example response:**
    ```
    SVG exported: 800x600, 3 signals, time range 0-100
    ```

## Standalone CLI

In addition to the MCP server, a standalone CLI tool `wave-analyzer-cli` is available for direct command-line access without an MCP client.

### CLI Usage

```bash
# Basic command
wave-analyzer-cli open_waveform /path/to/waveform.vcd

# Chain multiple commands with --
wave-analyzer-cli open_waveform test.vcd -- list_signals test.vcd --pattern clk

# Full workflow example
wave-analyzer-cli open_waveform test.vcd --alias mywave -- \\
  list_signals mywave -- \\
  read_signal mywave top.clk --time-indices 0,1,2,3 -- \\
  close_waveform mywave
```

### Python Agent Stdio JSON

For Python agents, prefer the long-lived stdio JSON mode instead of repeatedly
spawning one CLI process per operation:

```powershell
wave-analyzer-cli agent --stdio-json
```

This mode uses one JSON request and one JSON response per line, keeps waveform
state in the process, and returns stable `ok/data/error` envelopes. See
[`docs/PYTHON_AGENT_STDIO_JSON.md`](./docs/PYTHON_AGENT_STDIO_JSON.md) for the
Python subprocess wrapper and supported methods.

### CLI Commands

- **open_waveform** `<file_path>` [--alias `<name>`] - Open waveform file
- **close_waveform** `<id>` - Close waveform
- **list_signals** `<id>` [--pattern `<p>`] [--hierarchy `<h>`] [--recursive `<bool>`] [--limit `<n>`]
- **read_hierarchy** `<id>` [--scope `<scope>`] [--recursive `<bool>`] [--limit `<n>`]
- **read_signal** `<id>` `<signal>` [--time-index `<idx>` | --time-indices `<list>`]
- **get_signal_info** `<id>` `<signal>` - Get signal metadata
- **find_signal_events** `<id>` `<signal>` [--start `<idx>`] [--end `<idx>`] [--limit `<n>`]
- **find_conditional_events** `<id>` `<condition>` [--start `<idx>`] [--end `<idx>`] [--limit `<n>`]
- **load_dependencies** `<file_path>` [--alias `<name>`] - Load deps.yaml
- **load_assertion_log** `<file_path>` [--severity-filter `<list>`] [--limit `<n>`] - Parse transcript
- **load_design_spec** `<file_path>` [--alias `<name>`] - Load design_spec.yaml
- **trace_root_cause** `<waveform_id>` `<deps_id>` `<signal>` [--time-index `<idx>` | --time-value `<v>` --time-unit `<u>`] [--spec-id `<id>`] [--max-depth `<n>`]
- **find_fan_in** `<deps_id>` `<signal>` - Query upstream dependencies
- **find_fan_out** `<deps_id>` `<signal>` - Query downstream dependent signals
- **batch_trace_root_cause** `<waveform_id>` `<deps_id>` `<assertion_id>` [--spec-id `<id>`] [--max-depth `<n>`] [--severity-filter `<list>`] [--simulator `<name>`] - Batch BFS trace for all assertion events
- **export_bfs_report** `<trace_id>` [--format `<json|markdown|html>`] - Export BFS report
- **suggest_entry_signals** `<waveform_id>` `<deps_id>` [--assertion `<name>`] [--limit `<n>`]
- **extract_signal_values** `<waveform_id>` [--signal-path `<path>` | --bit-mapping `<json>`] [--format `<f>`]
- **analyze_handshake** `<waveform_id>` `<valid>` `<ready>` [--data-signal `<path>`] [--report-mode `<mode>`]
- **measure_signal** `<waveform_id>` `<signal>` [--analysis-type `<clock|pulse>`] [--edge-type `<posedge|negedge|both>`]
- **compare_signals** `<waveform_id>` `<signals_json>` [--mode `<all_equal|reference_vs_actual>`] [--format `<f>`]
- **multi_signal_timeline** `<waveform_id>` `<signals_json>` [--merge-mode `<union|intersection>`] [--format `<f>`]
- **auto_discover_signals** `<waveform_id>` [--mode `<bus_slices|clocks|groups|all>`] [--scope `<path>`] [--pattern `<regex>`]
- **detect_sequence** `<waveform_id>` `<conditions_json>` [--max-gap `<cycles>`]
- **compute_crc** `<waveform_id>` `<data_signal>` [--crc-signal `<path>`] [--polynomial `<crc-8|crc-16-ccitt|crc-32-ethernet>`]
- **load_run_summary** `<file_path>` [--alias `<name>`] - Parse run_summary.json and suggest next action
- **analyze_run** `<run_summary.json>` [--deps `<deps.yaml>`] [--spec `<design_spec.yaml>`] [--transcript `<transcript.log>`] [--waveform `<dump.vcd|dump.fst>`] [--severity-filter `<list>`] [--max-depth `<n>`] [--report-dir `<dir>`] [--report-format `<json|markdown|html|none>`] - Run end-to-end failure analysis orchestration
- **generate_summary** `<file_path>` [--signals `<list>`] [--max-samples `<n>`] - Generate waveform summary
- **export_svg** `<waveform_id>` [--signals `<list>`] [--width `<px>`] [--height `<px>`]


## Development

### Building

```bash
cargo build
cargo build --release
```

`extract_deps` 的单 exe 交付需要先生成内嵌 sidecar：

```bash
cd tools/deps-extractor
python -m pip install pyinstaller -r requirements.txt
python build_sidecar.py
cd ../..
cargo build --release --bin wave-analyzer-cli
```

Windows 维护构建也可以直接运行：

```powershell
.\scripts\build-wave-analyzer-cli-single-exe.ps1
```

构建时如果存在 `tools/deps-extractor/dist/wave-analyzer-deps-extractor.exe`，
`build.rs` 会自动把它嵌入 `wave-analyzer-cli.exe`。最终交付只需要一个
`wave-analyzer-cli.exe`，目标机器只要求 `iverilog` 已加入 Windows `PATH`。

> **Windows 用户**:运行 `wave-analyzer-cli help` 查看完整 PATH 配置
> (Vivado / iverilog / Python 3 + pyverilog)与 `setx` / 验证命令,
> 以及对应的环境变量( `IVERILOG_HOME`, `VIVADO_PATH`)。

### Testing

```bash
cargo test
```
## License

[MIT](LICENSE)

---

## 中文使用指南

> 本节为中文索引,与上方英文章节内容对照浓缩,方便国内团队与 AI 助手快速查阅。完整字段级说明仍以上方英文章节及 [`docs/`](./docs/) 为准。

### 简介

`wave-analyzer-mcp` 是基于 [wellen](https://github.com/ekiwi/wellen) 的 MCP(Model Context Protocol,模型上下文协议)服务器,用于读取与分析 VCD(Value Change Dump)/ FST(Fast Signal Trace)波形文件。可同时作为:

- **MCP 服务器**:供 AI 助手(如本仓库的 `../digital-assistant/`)通过工具调用消费;
- **独立 CLI**:`wave-analyzer-cli` 直接命令行使用,便于人工调试与脚本编排。

### 传输模式

| 模式 | 启动方式 | 适用场景 |
|------|---------|---------|
| **stdio**(默认) | `cargo run` 或 `wave-analyzer-mcp` | 本地 MCP 客户端(如 Claude Desktop),一对一对话 |
| **HTTP**(streamable) | `cargo run -- --http --bind-address 127.0.0.1:8000` | 多客户端共享同一份 waveform store(打开的波形 / 依赖图 / 断言日志 / 规范),供远程 Agent 调用 |

### 工具清单(27 个)

#### 基础波形工具(8 个)

| # | 工具 | 用途 |
|---|------|------|
| 1 | `open_waveform` | 打开 `.vcd` 或 `.fst` 文件,返回 `waveform_id` 或别名 |
| 2 | `close_waveform` | 关闭波形,释放内存 |
| 3 | `list_signals` | 列出信号(支持 `name_pattern` / `hierarchy_prefix` / `recursive` / `limit` 过滤) |
| 4 | `read_hierarchy` | 读模块层级树(仅 module scope) |
| 5 | `read_signal` | 读单个或多个时间索引下的信号值 |
| 6 | `get_signal_info` | 查询信号元数据(类型 / 位宽 / 索引范围) |
| 7 | `find_signal_events` | 查指定信号在某时间范围内的变化事件 |
| 8 | `find_conditional_events` | 用条件表达式(Verilog 风格)查找满足条件的时刻 |

#### 高级分析工具(8 个)

| # | 工具 | 用途 | 关键模块 |
|---|------|------|---------|
|  9 | `load_dependencies` | 加载 `deps.yaml` 依赖图,建立 fan-in/fan-out 索引与信号/时钟别名 | [`src/deps.rs`](./src/deps.rs) |
| 10 | `load_assertion_log` | 解析 ModelSim transcript(支持 vsim-10142/10143 两行格式、短单行、note 格式),按严重级别过滤 | [`src/assertion.rs`](./src/assertion.rs) |
| 11 | `load_design_spec` | (可选)加载 `design_spec.yaml`,映射断言到入口信号、提供调试提示与停止信号 | [`src/spec.rs`](./src/spec.rs) |
| 12 | `trace_root_cause` | BFS(Breadth-First Search,广度优先搜索)根因追踪:从失败信号沿 fan-in 反向展开 | [`src/bfs.rs`](./src/bfs.rs) |
| 13 | `find_fan_in` | 查询信号的直接上游依赖边(含 dependency type / clock / latency / protocol 信息) | [`src/deps.rs`](./src/deps.rs) |
| 14 | `find_fan_out` | 查询信号的直接下游依赖信号(正向影响分析:哪些输出受此信号影响) | [`src/deps.rs`](./src/deps.rs) |
| 15 | `batch_trace_root_cause` | 批量 BFS 根因追踪:对断言日志中所有失败事件逐一追踪,聚合根因候选 | [`src/bfs.rs`](./src/bfs.rs) |
| 16 | `export_bfs_report` | 导出 BFS 追踪结果为 JSON/Markdown/HTML 格式报告 | [`src/report.rs`](./src/report.rs) |
| 17 | `suggest_entry_signals` | 当无 `design_spec.yaml` 时,根据波形层级 + 依赖图智能推荐 BFS 入口信号(3 层优先级排序) | [`src/entry_signal.rs`](./src/entry_signal.rs) |

#### 波形摘要工具(2 个)

| # | 工具 | 用途 | 关键模块 |
|---|------|------|---------|
| 26 | `get_waveform_summary` | 生成波形统计摘要和信号采样(直接打开文件,不依赖 open_waveform) | [`src/summary.rs`](./src/summary.rs) |
| 27 | `export_waveform_svg` | 导出波形为 SVG 矢量图,支持指定信号列表、时间范围和尺寸 | [`src/summary.rs`](./src/summary.rs) |

#### 信号提取与协议分析工具(3 个)

| # | 工具 | 用途 | 关键模块 |
|---|------|------|---------|
| 18 | `extract_signal_values` | 提取信号值或从多个比特信号重建多比特信号 | [`src/extract.rs`](./src/extract.rs) |
| 19 | `analyze_handshake` | 分析 valid/ready 握手协议,计算延迟和吞吐 | [`src/protocol.rs`](./src/protocol.rs) |
| 20 | `measure_signal` | 测量时钟(周期/频率/占空比/抖动)或脉冲宽度统计 | [`src/protocol.rs`](./src/protocol.rs) |

#### 通用算法工具(5 个)

| # | 工具 | 用途 | 关键模块 |
|---|------|------|---------|
| 21 | `compare_signals` | 比较多个信号值,查找不匹配事件(支持 bit_mapping 重构) | [`src/compare.rs`](./src/compare.rs) |
| 22 | `multi_signal_timeline` | 构建 logic-analyzer 风格的多信号统一时间轴 | [`src/summary.rs`](./src/summary.rs) |
| 23 | `auto_discover_signals` | 自动发现波形中的总线分组、时钟和复位信号 | [`src/discovery.rs`](./src/discovery.rs) |
| 24 | `detect_sequence` | 检测有序条件序列,支持最大间隔约束 | [`src/sequence.rs`](./src/sequence.rs) |
| 25 | `compute_crc` | 计算数据信号的 CRC 并可选验证(observed CRC 信号) | [`src/crc.rs`](./src/crc.rs) |

### 模块组织

| 模块 | 职责 |
|------|------|
| [`src/main.rs`](./src/main.rs) / [`src/lib.rs`](./src/lib.rs) | 服务器入口、MCP 工具注册、waveform store 管理 |
| [`src/signal.rs`](./src/signal.rs) | 信号列出、值读取、事件查找、元数据查询 |
| [`src/hierarchy.rs`](./src/hierarchy.rs) | 波形模块层级树遍历 |
| [`src/condition.rs`](./src/condition.rs) + [`src/condition.lalrpop`](./src/condition.lalrpop) | 条件表达式解析器(Verilog 风格,含 `$past` / 位提取 / Verilog 字面量) |
| [`src/bfs.rs`](./src/bfs.rs) | BFS 根因追踪引擎(支持多周期延迟、边界检测、循环检测、批量追踪) |
| [`src/deps.rs`](./src/deps.rs) | 依赖图加载与 fan-in/fan-out 查询 |
| [`src/assertion.rs`](./src/assertion.rs) | ModelSim 断言日志解析 |
| [`src/spec.rs`](./src/spec.rs) | 设计规范 YAML 查询 |
| [`src/entry_signal.rs`](./src/entry_signal.rs) | 入口信号智能推荐 |
| [`src/report.rs`](./src/report.rs) | BFS 报告导出(JSON/Markdown/HTML) |
| [`src/time_map.rs`](./src/time_map.rs) | 时钟边表构建、时间索引与值之间的映射 |
| [`src/protocol.rs`](./src/protocol.rs) | 握手协议分析与时钟/脉冲测量 |
| [`src/compare.rs`](./src/compare.rs) | 多信号比较与不匹配检测 |
| [`src/discovery.rs`](./src/discovery.rs) | 信号模式自动发现(总线分组/时钟/复位) |
| [`src/sequence.rs`](./src/sequence.rs) | 有序条件序列检测 |
| [`src/crc.rs`](./src/crc.rs) | CRC 计算与验证 |
| [`src/extract.rs`](./src/extract.rs) | 信号值提取与多比特重建 |
| [`src/formatting.rs`](./src/formatting.rs) | 信号值与时间值格式化(带 timescale) |
| [`src/cli_parser.rs`](./src/cli_parser.rs) | `wave-analyzer-cli` 的参数解析(支持 `--` 链式调用) |

两个 binary:`wave-analyzer-mcp`(MCP 服务器)与 `wave-analyzer-cli`(独立 CLI 工具)。

### 条件表达式语法速查

`find_conditional_events` 使用 LALRPOP 生成的解析器,支持 Verilog 风格条件表达式。运算符优先级(从高到低):

1. `~`(按位 NOT)、`!`(逻辑 NOT)
2. `==`、`!=`(等于 / 不等于)
3. `&`(按位 AND)
4. `^`(按位 XOR)
5. `|`(按位 OR)
6. `&&`(逻辑 AND)
7. `||`(逻辑 OR)

特殊语法:

- `$past(signal)` —— 读取上一时刻值
- `signal[bit]` / `signal[msb:lsb]` —— 位提取(单 bit 或范围)
- Verilog 字面量:`4'b0101`(二进制)、`3'd2`(十进制)、`5'h1A`(十六进制)

常用示例:

```text
TOP.valid && TOP.ready                  # handshake(握手)成立的时刻
TOP.counter == 4'd10                    # 计数器等于 10
!$past(TOP.signal) && TOP.signal        # 上升沿(rising edge)
$past(TOP.signal) && !TOP.signal        # 下降沿(falling edge)
TOP.flags & 4'b0001                     # 按位 AND,检查 bit 0 是否置位
(TOP.valid && TOP.data != 8'hFF) || TOP.error
```

### YAML 配置文件指引

`load_dependencies` / `load_design_spec` 接受 YAML 文件,格式定义在 [`docs/`](./docs/) 下:

- 依赖图格式: [`docs/DEPS_FORMAT.md`](./docs/DEPS_FORMAT.md) —— `deps.yaml` 的节点 / 边 / 别名 / 时钟语义
- 设计规范格式: [`docs/DESIGN_SPEC_FORMAT.md`](./docs/DESIGN_SPEC_FORMAT.md) —— `design_spec.yaml` 关联断言到入口信号和调试提示
- 端到端样例: [`docs/MINIMAL_REFERENCE_EXAMPLE.md`](./docs/MINIMAL_REFERENCE_EXAMPLE.md) —— 完整的 VCD + deps + spec + trace 流程

### 典型工作流(BFS 根因追踪)

```bash
# 1. 打开波形
wave-analyzer-cli open_waveform dump.vcd --alias mywave -- \
  # 2. 加载依赖图(由 RTL 静态分析或手工标注产出)
  load_dependencies deps.yaml --alias mydeps -- \
  # 3. 解析 ModelSim 断言日志,定位失败时刻
  load_assertion_log transcript.log --severity-filter Error,Failure -- \
  # 4. (可选)加载设计规范,自动提供入口信号
  load_design_spec design_spec.yaml --alias myspec -- \
  # 5. BFS 反向追踪,从失败信号沿 fan-in 展开
  trace_root_cause mywave mydeps TOP.failed_signal \
    --time-value 30 --time-unit ns --spec-id myspec --max-depth 8
```

整体流程图与算法设计见 [`docs/WORKFLOW_DESIGN.md`](./docs/WORKFLOW_DESIGN.md) 与 [`docs/BFS_ENGINE_DESIGN.md`](./docs/BFS_ENGINE_DESIGN.md)。

### 测试与开发

```bash
# 跑全部 18 个测试文件
cargo test

# 单测试文件(如 condition_tests / bfs_tests / deps_tests)
cargo test --test condition_tests

# debug 模式快速运行(stdio 传输)
sh run.sh
```

注意:`build.rs` 调用 `lalrpop::process_root()`,若修改 [`src/condition.lalrpop`](./src/condition.lalrpop) 语法文件,下一次 `cargo build` 会自动重新生成解析器代码。

### 作为 digital-assistant 的 Layer 4a

本服务被同仓库 [`../digital-assistant/`](../digital-assistant/) 作为 **Layer 4a 基础设施**消费:

- 通信协议:MCP over HTTP(streamable),端点 `127.0.0.1:8000/mcp`;
- 启动方式:VSCode 扩展通过子进程拉起 `wave-analyzer-mcp.exe --http`;
- 能力发现:握手后 Agent 调用 `tools/list` 动态拉取全部工具 schema;
- 能力分层:`waveform-core` 7 个基础工具是 M1 最低验收边界;`waveform-advanced` 其余工具按需启用。

集成细节见 [`../digital-assistant/docs/DESIGN.md`](../digital-assistant/docs/DESIGN.md) §3.5 与 §4.6,以及 ADR-0004(接口权威源)。

### 更多文档

[`docs/`](./docs/) 目录下的设计文档(中文):

| 文档 | 用途 |
|------|------|
| [`WORKFLOW_DESIGN.md`](./docs/WORKFLOW_DESIGN.md) | 波形分析系统总体工作流(打开波形 → 加载依赖 → 解析断言 → BFS 追踪) |
| [`BFS_ENGINE_DESIGN.md`](./docs/BFS_ENGINE_DESIGN.md) | BFS 根因分析引擎设计(输入输出、节点状态、候选排序) |
| [`DEPS_FORMAT.md`](./docs/DEPS_FORMAT.md) | 信号依赖图 YAML 格式规范 |
| [`DESIGN_SPEC_FORMAT.md`](./docs/DESIGN_SPEC_FORMAT.md) | 设计需求文档 YAML 格式规范 |
| [`MINIMAL_REFERENCE_EXAMPLE.md`](./docs/MINIMAL_REFERENCE_EXAMPLE.md) | 端到端最小样例(VCD + deps + spec + trace) |
| [`INTERFACE_CONTRACTS.md`](./docs/INTERFACE_CONTRACTS.md) | MCP / CLI 工具的输入输出契约 |
| [`SIM_SCRIPTS_DESIGN.md`](./docs/SIM_SCRIPTS_DESIGN.md) | Vivado + ModelSim 仿真脚本设计(产出 `run_summary.json` / `transcript.log` / `dump.vcd`) |
| [`DEPS_EXTRACTION_DESIGN.md`](./docs/DEPS_EXTRACTION_DESIGN.md) | RTL 依赖图自动提取方案(Vivado Tcl + Pyverilog 双引擎,从 RTL 源码生成 `deps.yaml`) |
| [`BFS_ALGORITHM_GUIDE.md`](./docs/BFS_ALGORITHM_GUIDE.md) | BFS 根因追溯算法入门教程 |

