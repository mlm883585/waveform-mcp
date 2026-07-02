# 接口契约文档

## 1. 目标

本文档定义 `wave-analyzer-mcp`、`wave-analyzer-cli` 与仿真脚本之间的接口契约，重点解决以下问题：

1. 未来接口实现时，输入输出格式保持稳定。
2. AI、CLI、MCP、仿真脚本之间使用同一套字段语义。
3. 后续功能实现前，先固定“请求长什么样、响应长什么样、错误如何表示”。

本文档面向“计划接口”，不表示当前仓库已经实现这些接口。

---

## 2. 适用范围

| 范围 | 说明 |
|---|---|
| MCP Tool 接口 | AI 通过 MCP 调用的工具契约 |
| CLI 接口 | 用户或脚本直接调用的命令行契约 |
| 仿真摘要文件 | `run_summary.json` 的结构契约 |
| 通用错误模型 | CLI/MCP 共享的错误语义 |

---

## 3. 设计原则

| 原则 | 说明 |
|---|---|
| 契约先于实现 | 先固定字段，再写代码 |
| 同名同义 | 同一概念在 CLI/MCP/JSON 中尽量同名 |
| 结构化优先 | 优先输出可机读结构，再附加文本摘要 |
| 兼容相控阵场景 | 支持多通道 canonical 命名、别名映射、批量分析 |
| 错误可分流 | 编译失败、解析失败、图缺失、波形缺失必须可区分 |

### 3.1 输出模型实现策略

**第一版不建独立 `src/output.rs` 模块。** 原因：

1. 现有 MCP 工具统一使用 `rmcp` 的 `CallToolResult`（`Content::text(json_str)`），新增工具沿用同一模式，不引入新的响应类型系统。
2. CLI 工具直接输出文本或 `serde_json::to_string_pretty` 格式化 JSON，无需共享抽象层。
3. 错误码先用 `String` 描述，后续可抽成 enum。

**MCP 输出规则:**

| 场景 | 实现方式 |
|---|---|
| 成功响应 | `CallToolResult::success(vec![Content::text(json_str)])`，`json_str` 字段按本文档 §6 契约序列化 |
| 失败响应 | `CallToolResult::error(vec![Content::text(error_json_str)])`，`error_json_str` 按 §6.2 契约序列化 |
| 同时提供摘要 | JSON 中包含 `summary` 字段作为文本摘要，不再额外拼自然语言 |

**CLI 输出规则:**

| 场景 | 实现方式 |
|---|---|
| `--format text` | 打印首行摘要 + 关键字段列表 |
| `--format json` | 用 `serde_json::to_string_pretty(&result)` 输出结构化 JSON，字段名与 MCP 一致 |
| 默认 | 新命令默认 `text`，与现有 CLI 风格一致 |

**后续迭代:**

若后续发现 MCP 和 CLI 的响应序列化逻辑大量重复，可考虑抽取 `src/output.rs` 做公共 serde struct + 序列化 helper。但第一版优先追求功能闭环，不追求抽象统一。

---

## 4. 通用命名约定

| 字段 | 含义 |
|---|---|
| `waveform_id` | 已加载波形的别名 |
| `deps_id` | 已加载依赖图的 ID（别名） |
| `signal_path` | canonical 信号路径，优先使用 `TOP.xxx` 风格 |
| `resolved_signal_path` | 映射到实际波形中的路径 |
| `time_index` | 波形时间表索引 |
| `time_value` | 原始时间值 |
| `time_unit` | `ps/ns/us/ms` 等 |
| `time_ps` | 统一换算后的皮秒值 |
| `clock_name` | 参考时钟名称 |
| `latency_cycles` | 相对参考时钟的周期数 |
| `status` | 当前调用或节点状态 |

---

## 5. MCP 工具总览

全部 27 个 MCP 工具及 30+ CLI 命令已实现，完整参数说明与示例见 `README.md`。

> 注：设计文档中 `parse_assertion_log` 在实际实现时更名为 `load_assertion_log`，其余工具名与设计一致。

---

## 6. MCP 通用响应契约

### 6.1 成功响应

实际实现使用 `rmcp` 框架的 `CallToolResult`，包含一个或多个 `Content` 项：

```rust
CallToolResult::success(vec![
    Content::text(summary_text),       // 人类可读摘要
    Content::json(structured_data),    // 结构化 JSON（可选）
])
```

`Content::text` 提供人类可读的摘要文本；`Content::json` 提供可机读的结构化数据（如 BfsResult 的 serde 序列化）。下方各工具契约的 JSON 示例展示 `Content::json` 的内容结构。

### 6.2 失败响应

```rust
CallToolResult::error(vec![
    Content::text(error_description),
])
```

错误描述为自然语言文本，包含错误码和具体原因。

### 6.3 错误码分类

| 错误码 | 含义 |
|---|---|
| `FILE_NOT_FOUND` | 文件不存在 |
| `WAVEFORM_NOT_FOUND` | 波形别名不存在 |
| `DEPS_NOT_FOUND` | 依赖图别名不存在 |
| `ASSERTION_LOG_PARSE_ERROR` | transcript 解析失败 |
| `DEPS_PARSE_ERROR` | `deps.yaml` 解析失败 |
| `SIGNAL_NOT_FOUND` | 信号找不到 |
| `CLOCK_NOT_FOUND` | BFS 所需参考时钟找不到 |
| `TIME_MAPPING_ERROR` | 时间映射失败 |
| `INVALID_ARGUMENT` | 参数非法 |
| `INTERNAL_ERROR` | 内部处理错误 |

---

## 7. `load_assertion_log` 契约

### 7.1 MCP 请求

```json
{
  "file_path": "sim_output/transcript.log",
  "alias": "assert_log_1",
  "severity_filter": ["error", "warning"],
  "limit": 100
}
```

> **注意:** 实际代码参数名为 `file_path`（而非旧文档中的 `transcript_path`），与所有 `load_*` 工具保持一致。

### 7.2 MCP 响应

```json
{
  "ok": true,
  "tool": "load_assertion_log",
  "data": {
    "events": [
      {
        "assertion_name": "assert_coeff_valid_latency",
        "severity": "error",
        "scope_path": "tb_top",
        "time_value": 1750,
        "time_unit": "ns",
        "time_ps": 1750000,
        "source_file": "tb/tb_top.sv",
        "source_line": 42
      }
    ],
    "total": 1
  },
  "summary": "共解析到 1 条断言事件"
}
```

### 7.3 CLI 形式

```powershell
wave-analyzer-cli load_assertion_log sim_output/transcript.log --alias assert_log_1 --severity error,warning --limit 100
```

### 7.4 CLI 输出建议

优先支持两种模式：

| 模式 | 说明 |
|---|---|
| 文本摘要 | 便于人工查看 |
| JSON | 便于脚本和 AI 消费 |

JSON 示例：

```json
{
  "events": [
    {
      "assertion_name": "assert_coeff_valid_latency",
      "severity": "error",
      "scope_path": "tb_top",
      "time_value": 1750,
      "time_unit": "ns",
      "time_ps": 1750000
    }
  ]
}
```

---

## 8. `load_dependencies` 契约

### 8.1 MCP 请求

```json
{
  "file_path": "specs/deps.yaml",
  "alias": "beam_deps",
  "simulator": "modelsim"
}
```

> **注意:** 实际代码参数名为 `file_path`（而非旧文档中的 `deps_file`），与所有 `load_*` 工具保持一致。别名参数为 `alias`，在其他工具中引用时使用 `deps_id`。

### 8.2 MCP 响应

```json
{
  "ok": true,
  "tool": "load_dependencies",
  "data": {
    "deps_id": "beam_deps",
    "node_count": 128,
    "edge_count": 244,
    "has_cycles": true,
    "signal_alias_count": 64,
    "clock_alias_count": 3
  },
  "summary": "依赖图加载成功，128 个节点，244 条边"
}
```

### 8.3 CLI 形式

```powershell
wave-analyzer-cli load_dependencies specs/deps.yaml --alias beam_deps --simulator modelsim
```

### 8.4 时钟解析契约

`load_dependencies` 应同时加载 `signal_aliases` 与 `clock_aliases`。

其中：

1. `signal_aliases` 负责 canonical 业务信号到实际波形路径的映射。
2. `clock_aliases` 负责逻辑时钟名到实际波形时钟路径的映射。
3. `trace_root_cause` 不应直接把 `dep.clock` 当作波形路径使用，而应先经 `clock_aliases` 解析。

### 8.5 信号别名与波形的运行时关联

`load_dependencies` 和 `open_waveform` 是两个独立的 store，加载时不互相依赖。别名解析在 BFS 执行时动态完成。

**解析流程:**

1. `load_dependencies` 加载 `deps.yaml`，将 `signal_aliases` 和 `clock_aliases` 存入 `DepGraph` 结构。`DepGraph` 不持有波形引用。
2. `trace_root_cause` 被调用时，同时需要 `waveform_id` 和 `deps_id`。
3. BFS 内部对每条 `DepEdge` 的 `signal` / `clock` 字段执行运行时别名解析：
   - 从 `DepGraph` 取 canonical 信号名对应的 `modelsim` 路径。
   - 用 `waveform.hierarchy()` 查找该路径是否存在。
   - 如果路径不存在，返回 `SIGNAL_NOT_FOUND` 错误。
   - 如果路径存在，返回 `VarRef` / `SignalRef` 用于后续信号读取。
4. 时钟别名的解析同理：从 `DepGraph` 取逻辑时钟名对应的 `modelsim` 路径，再在波形中查找。

**关键约束:**

- `load_dependencies` 的返回信息中应包含 `signal_alias_count` 和 `clock_alias_count`，但不验证别名是否在波形中可找到（因为加载时波形可能尚未打开）。
- 别名路径验证推迟到 `trace_root_cause` 执行时，这样允许先加载 deps 再打开波形，顺序灵活。
- 如果多个波形中存在同一 canonical 信号的不同 resolved 路径，`trace_root_cause` 只使用 `waveform_id` 指定的波形做解析，不会跨波形搜索。

---

## 9. `find_fan_in` / `find_fan_out` 契约

### 9.1 MCP 请求

```json
{
  "deps_id": "beam_deps",
  "signal_path": "TOP.ch0.beam_data_o",
  "max_depth": 3
}
```

### 9.2 MCP 响应

```json
{
  "ok": true,
  "tool": "find_fan_in",
  "data": {
    "root_signal": "TOP.ch0.beam_data_o",
    "direction": "fan_in",
    "nodes": [
      {
        "signal_path": "TOP.ch0.beam_data_o",
        "depth": 0
      },
      {
        "signal_path": "TOP.ch0.data_pipe3",
        "depth": 1,
        "edge_type": "sequential",
        "clock_name": "clk_sys",
        "latency_cycles": 1
      }
    ]
  },
  "summary": "共展开 2 个节点"
}
```

### 9.3 CLI 形式

```powershell
wave-analyzer-cli find_fan_in beam_deps TOP.ch0.beam_data_o --depth 3
wave-analyzer-cli find_fan_out beam_deps TOP.cfg_valid --depth 4
```

---

## 10. `trace_root_cause` 契约

### 10.1 MCP 请求

```json
{
  "waveform_id": "beam_wave",
  "signal_path": "TOP.ch0.beam_data_o",
  "time_index": 175,
  "deps_id": "beam_deps",
  "max_depth": 8,
  "stop_signals": ["TOP.cfg_valid", "TOP.cfg_data"],
  "enable_auto_check": true,
  "format": "tree"
}
```

### 10.2 请求字段说明

| 字段 | 必选 | 说明 |
|---|---|---|
| `waveform_id` | 是 | 已加载波形 |
| `signal_path` | 是 | BFS 入口信号 |
| `time_index` | 是* | 故障时刻的波形时间表索引 |
| `time_value` | 否* | 故障时刻的原始时间值（如 `1750`），配合 `time_unit` 使用 |
| `time_unit` | 否* | 时间单位（`ps/ns/us/ms`），配合 `time_value` 使用 |
| `deps_id` | 是 | 已加载依赖图 ID（load_dependencies 返回的 alias） |
| `max_depth` | 否 | 默认深度限制 |
| `stop_signals` | 否 | 人工边界 |
| `enable_auto_check` | 否 | 是否启用边检查 |
| `format` | 否 | `json` / `text` |

> **时间输入规则:** `time_index` 和 `time_value+time_unit` 二选一。若同时提供，优先使用 `time_index`。第一版实现必须支持 `time_index`；`time_value+time_unit` 作为可选扩展，但接口字段应预留。

### 10.2.1 多入口信号处理规则

`trace_root_cause` 一次只追一个入口信号。

当同一个失败事件对应多个候选入口时，推荐上层按以下顺序处理：

1. 若 spec 已给出 `fail_entry_signals[0]` 或主入口约定，优先使用该信号。
2. 若来自断言失败，则优先选择 `observe_signals` 中最接近失败现象的输出/状态信号作为第一次 BFS 入口。
3. 若第一次 BFS 只能得到上下文节点、无法收敛，可对剩余入口信号逐个重试，并在报告中保留“入口来源”。

推荐编排：

```text
AssertionEvent
  -> observe_signals[]
  -> choose_primary_entry()
  -> trace_root_cause(primary_entry, time_index)
  -> optional retry on secondary entries
```

### 10.3 MCP 响应

> **注意:** 实际 MCP 返回使用 `rmcp` 框架的 `CallToolResult`，包含 `Content::text`（人类可读摘要）和可选 `Content::json`（结构化数据）。下方 JSON 示例展示 `Content::json` 的内容结构。

```json
{
  "ok": true,
  "tool": "trace_root_cause",
  "data": {
    "root_signal": "TOP.ch0.beam_data_o",
    "root_time_index": 175,
    "root_time_ps": 1750000,
    "tree": [
      {
        "node_id": "n0",
        "signal_path": "TOP.ch0.beam_data_o",
        "resolved_signal_path": "TOP.gen_ch__0.beam_data_o",
        "time_index": 175,
        "time_ps": 1750000,
        "depth": 0,
        "status": "Suspect",
        "actual_value": "16'h0000"
      },
      {
        "node_id": "n1",
        "parent_id": "n0",
        "signal_path": "TOP.ch0.data_pipe3",
        "resolved_signal_path": "TOP.gen_ch__0.data_pipe3",
        "time_index": 174,
        "time_ps": 1745000,
        "depth": 1,
        "status": "Ok",
        "edge_type": "sequential",
        "clock_name": "clk_sys",
        "latency_cycles": 1,
        "actual_value": "16'h0000"
      }
    ],
    "candidates": [
      {
        "signal_path": "TOP.ch0.coeff_valid",
        "time_index": 172,
        "time_ps": 1735000,
        "status": "RootCauseCandidate",
        "reason": "关键控制链路异常，且下游数据路径检查通过"
      }
    ]
  },
  "summary": "根因追溯完成，发现 1 个高优先级候选节点"
}
```

### 10.4 CLI 形式

```powershell
wave-analyzer-cli trace_root_cause beam_wave TOP.ch0.beam_data_o 175 --deps-id beam_deps --depth 8 --format json
```

---

## 10.5 `batch_trace_root_cause` 契约

### MCP 请求

```json
{
  "waveform_id": "beam_wave",
  "deps_id": "beam_deps",
  "assertion_id": "assert_log_1",
  "spec_id": "spec_1",
  "max_depth": 8,
  "severity_filter": "Error,Failure",
  "simulator": "modelsim"
}
```

### 请求字段说明

| 字段 | 必选 | 说明 |
|---|---|---|
| `waveform_id` | 是 | 已加载波形 |
| `deps_id` | 是 | 已加载依赖图 ID |
| `assertion_id` | 是 | 已加载断言日志 ID（来自 load_assertion_log） |
| `spec_id` | 否 | 已加载设计规格 ID，用于入口信号解析 |
| `max_depth` | 否 | 单次 BFS 最大深度（默认 8） |
| `severity_filter` | 否 | 严重性过滤，逗号分隔（默认全部） |
| `simulator` | 否 | 仿真器名称，用于别名解析（默认 "modelsim"） |

### MCP 响应

```json
{
  "traces": [
    {
      "assertion_name": "ASSERT_DATA",
      "entry_signal": "TOP.data_out",
      "fail_time_ps": 1750000,
      "result": { /* BfsResult 结构 */ }
    }
  ],
  "aggregated_candidates": [ /* RootCauseCandidate[] */ ],
  "summary": "Batch trace complete. 2 traces, 1 unique root cause candidate."
}
```

### CLI 形式

```powershell
wave-analyzer-cli batch_trace_root_cause beam_wave --deps-id beam_deps --assertion-id assert_log_1 --spec-id spec_1 --severity Error,Failure --depth 8
```

---

## 10.6 `export_bfs_report` 契约

### MCP 请求

```json
{
  "trace_id": "trace_1",
  "format": "json"
}
```

### 请求字段说明

| 字段 | 必选 | 说明 |
|---|---|---|
| `trace_id` | 是 | trace_root_cause 返回的 trace ID |
| `format` | 否 | 输出格式：`json`、`markdown`、`html`（默认 `json`） |

### MCP 响应

返回对应格式的完整报告文本。JSON 格式返回 BfsResult 的 `serde_json` 序列化；Markdown 返回文本摘要 + trace tree；HTML 返回带 CSS 样式的完整 HTML 页面。

### CLI 形式

```powershell
wave-analyzer-cli export_bfs_report trace_1 --format markdown
wave-analyzer-cli export_bfs_report trace_1 --format html
```

---

## 10.7 `load_run_summary` 契约

### MCP 请求

```json
{
  "file_path": "sim_output/run_summary.json",
  "alias": "run_1"
}
```

### 请求字段说明

| 字段 | 必选 | 说明 |
|---|---|---|
| `file_path` | 是 | run_summary.json 文件路径 |
| `alias` | 否 | 存储别名（默认取文件名） |

### MCP 响应

```json
{
  "status": "assertion_failed",
  "project_name": "beam_project",
  "top_module": "tb_top",
  "compile_ok": true,
  "elab_ok": true,
  "simulation_ok": true,
  "assertion_fail_count": 2,
  "warning_count": 1,
  "error_count": 2,
  "wave_file": "sim_output/dump.vcd",
  "wave_format": "vcd",
  "transcript_file": "sim_output/transcript.log",
  "simulator": "modelsim",
  "finished_at": "2026-05-09T10:20:30",
  "next_step": "Assertion failures detected. Use load_assertion_log + trace_root_cause for root cause analysis."
}
```

> **注意:** `compile_ok`、`elab_ok`、`simulation_ok` 既接受 JSON boolean (`true`/`false`)，也接受字符串 `"true"`/`"false"`（兼容 PowerShell ConvertTo-Json 输出）。详见 §13。

### CLI 形式

```powershell
wave-analyzer-cli load_run_summary sim_output/run_summary.json --alias run_1
```

---

## 10.8 `get_waveform_summary` 契约

### MCP 请求

```json
{
  "file_path": "sim_output/dump.vcd",
  "signals": ["TOP.clk", "TOP.data_out"],
  "max_samples": 100
}
```

### 请求字段说明

| 字段 | 必选 | 说明 |
|---|---|---|
| `file_path` | 是 | 波形文件路径（VCD/FST） |
| `signals` | 否 | 要摘要的信号路径列表（空=自动检测顶层信号） |
| `max_samples` | 否 | 每信号最大采样数（默认 100） |

### CLI 形式

```powershell
wave-analyzer-cli get_waveform_summary sim_output/dump.vcd --signals TOP.clk,TOP.data_out --max-samples 100
```

---

## 10.9 `export_waveform_svg` 契约

### MCP 请求

```json
{
  "waveform_id": "beam_wave",
  "signals": ["TOP.clk", "TOP.data_out"],
  "time_range": [0, 500],
  "width": 800
}
```

### 请求字段说明

| 字段 | 必选 | 说明 |
|---|---|---|
| `waveform_id` | 是 | 已加载波形 ID |
| `signals` | 否 | 要渲染的信号路径列表 |
| `time_range` | 否 | 时间范围 `(start, end)`，时间索引 |
| `width` | 否 | 输出图像宽度（像素，默认 800） |

### MCP 响应

返回 SVG 文本内容（Content::text），包含波形渲染的可缩放矢量图形。

### CLI 形式

```powershell
wave-analyzer-cli export_waveform_svg beam_wave --signals TOP.clk,TOP.data_out --time-range 0,500 --width 800
```

---

## 11. 已有波形工具的兼容约定

为减少接口割裂，未来新工具应尽量与现有工具保持一致：

| 现有模式 | 新接口建议 |
|---|---|
| `waveform_id` | 保持不变 |
| `signal_path` | 保持不变 |
| 时间范围 `start/end` | 新接口优先使用同一命名风格 |
| 结果默认文本 | 保留文本摘要，同时增加 JSON 模式 |

---

## 12. CLI 输出模式契约

### 12.1 通用选项建议

未来计划接口建议统一支持：

```text
--format text
--format json
```

### 12.2 文本模式要求

文本输出必须满足：

1. 首行给出结论摘要。
2. 后续给出关键字段。
3. 不把结构化字段压缩成不可解析的大段自然语言。

### 12.3 JSON 模式要求

JSON 模式必须：

1. 可被脚本直接解析。
2. 字段名与 MCP 保持一致。
3. 不混入解释性噪音文本。

---

## 12.4 `time_value → time_index` 映射函数

BFS 和上层编排需要把 transcript 中提取的 `time_value + time_unit` 映射为波形的 `time_index`。

**实现规范:**

```rust
/// 将物理时间值映射到波形 time_table 中最近的 time_index。
///
/// # 参数
/// - `waveform`: 已加载的波形对象
/// - `time_ps`: 统一换算后的皮秒值
///
/// # 返回
/// - 找到完全匹配时：返回对应 time_index
/// - 无完全匹配时：返回最近不晚于 time_ps 的 time_index
/// - time_ps 超出波形范围时：返回最后一个 time_index
///
/// # 映射规则
/// 1. 将输入 time_value 按时间单位统一换算为 ps
/// 2. 从波形 timescale 将 time_table 值也换算为 ps
/// 3. 二分搜索 time_table 找到最近不晚于目标 ps 的索引
fn find_time_index_by_value(waveform: &Waveform, time_ps: u64) -> usize;
```

**上层编排换算规则:**

| 输入单位 | 换算为 ps 的系数 |
|---|---|
| `ps` | × 1 |
| `ns` | × 1000 |
| `us` | × 1000000 |
| `ms` | × 1000000000 |

**调用位置:**

1. `trace_root_cause` MCP/CLI handler：当调用者提供 `time_value+time_unit` 而非 `time_index` 时，先调用此函数映射。
2. AI 编排层：从 `parse_assertion_log` 得到 `time_value+time_unit` 后，调用此函数映射再传入 BFS。

**函数归属:**

建议放入 `src/time_map.rs` 作为公共 helper，通过 `lib.rs` 导出。BFS 和 handler 均可复用。

---

## 13. `run_summary.json` 契约

### 13.1 最小字段集

```json
{
  "status": "assertion_failed",
  "compile_ok": true,
  "elab_ok": true,
  "simulation_ok": true,
  "assertion_fail_count": 2,
  "wave_file": "sim_output/dump.vcd",
  "transcript_file": "sim_output/transcript.log",
  "top_module": "tb_top"
}
```

### 13.2 建议扩展字段

```json
{
  "status": "assertion_failed",
  "project_name": "beam_project",
  "compile_ok": true,
  "elab_ok": true,
  "simulation_ok": true,
  "assertion_fail_count": 2,
  "warning_count": 1,
  "error_count": 2,
  "wave_file": "sim_output/dump.vcd",
  "wave_format": "vcd",
  "transcript_file": "sim_output/transcript.log",
  "top_module": "tb_top",
  "simulator": "modelsim_20_1_1_720",
  "finished_at": "2026-05-09T10:20:30+08:00"
}
```

### 13.3 状态枚举

| 状态值 | 含义 |
|---|---|
| `compile_failed` | 编译失败 |
| `elab_failed` | 加载/展开失败 |
| `simulation_failed` | 仿真异常中止 |
| `assertion_failed` | 存在 error 级断言失败 |
| `passed` | 仿真完成且通过 |

---

## 14. AI 调度契约

### 14.1 调度输入

AI 自动分析最少需要以下输入：

| 输入 | 来源 |
|---|---|
| `run_summary.json` | 仿真脚本 |
| `design_spec.yaml` | 规格文件 |
| `deps.yaml` | 依赖图 |
| `transcript.log` | 断言日志 |
| `dump.vcd` | 波形文件 |

### 14.2 调度顺序

| 条件 | 下一步 |
|---|---|
| `status=compile_failed` | 不进入 BFS，先处理编译问题 |
| `status=assertion_failed` | 解析 transcript，按 spec 找入口，再调用 BFS |
| `status=passed` | 不继续分析 |

---

## 15. 版本兼容建议

### 15.1 第一版范围

第一版接口建议只承诺：

1. ModelSim 20.1.1.720 transcript 格式。
2. `simulator=modelsim`。
3. `time_index` 作为 BFS 主输入时间。
4. JSON 与文本两种输出模式。

### 15.2 后续扩展

| 扩展项 | 说明 |
|---|---|
| 多仿真器别名 | 预留 `vivado`、其他仿真器映射 |

---

## 16. 与其他文档的关系

| 文档 | 关系 |
|---|---|
| `WORKFLOW_DESIGN.md` | 定义接口在总流程中的位置 |
| `SIM_SCRIPTS_DESIGN.md` | 提供 `run_summary.json` 与 transcript 来源 |
| `DESIGN_SPEC_FORMAT.md` | 提供 BFS 入口信号字段来源 |
| `DEPS_FORMAT.md` | 提供 `load_dependencies` 和 BFS 所需图字段 |
| `BFS_ENGINE_DESIGN.md` | 提供 `trace_root_cause` 的算法语义 |

本文档负责把这些文档中的抽象设计收敛为统一的输入输出契约。
