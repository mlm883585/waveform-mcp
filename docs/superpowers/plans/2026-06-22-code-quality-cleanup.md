# 代码质量清理 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 清理 v0.5.0 错误重构后遗留的 4 处代码质量问题

**架构：** 4 个独立小改动：ReportWriter 迁移、panic 修复、依赖清理、server handler 错误类型升级

**技术栈：** Rust, thiserror v2, rmcp MCP framework, ReportWriter macros

---

## 文件结构

| 文件 | 职责 | 改动类型 |
|---|---|---|
| `src/fsm.rs` | FSM 提取，`generate_dot_graph` 函数 | 修改 §1 |
| `src/entry_signal.rs` | 入口信号推荐，`fan_in().unwrap()` | 修改 §2 |
| `Cargo.toml` | 依赖声明 | 修改 §3 |
| `src/server/mod.rs` | MCP 服务主入口，`main()` | 修改 §3 |
| `src/server/waveform_tools.rs` | 波形管理 handler | 修改 §4 |
| `src/server/cdc_tools.rs` | CDC 分析 handler | 修改 §4 |
| `src/server/protocol_tools.rs` | 协议分析 handler | 修改 §4 |
| `src/server/analysis_tools.rs` | BFS/入口信号 handler | 修改 §4 |
| `src/server/report_tools.rs` | 报告导出 handler | 修改 §4 |

---

### 任务 1：generate_dot_graph 转 ReportWriter

**文件：**
- 修改：`src/fsm.rs:388-423`

- [ ] **步骤 1：修改 generate_dot_graph 函数**

将 `String` + `writeln!().unwrap()` 替换为 `ReportWriter` + `report_writeln!`：

```rust
fn generate_dot_graph(
    states: &[FsmState],
    transitions: &[FsmTransition],
    signal_path: &str,
) -> String {
    let mut dot = ReportWriter::new();
    report_writeln!(dot, "digraph FSM_{} {{", sanitize_dot_name(signal_path));
    report_writeln!(dot, "  rankdir=LR;");
    report_writeln!(dot, "  node [shape=circle];");

    for state in states {
        let label = format!("{}\\n{}", state.name, state.value);
        report_writeln!(
            dot,
            "  {} [label=\"{}\"];",
            sanitize_dot_name(&state.name),
            label
        );
    }

    for trans in transitions {
        let label = format!("{}\\navg={:.1}", trans.count, trans.duration_stats.avg);
        report_writeln!(
            dot,
            "  {} -> {} [label=\"{}\"];",
            sanitize_dot_name(&trans.from_state),
            sanitize_dot_name(&trans.to_state),
            label
        );
    }

    report_writeln!(dot, "}}");
    dot.finish()
}
```

同时删除 `use std::fmt::Write as _;`（已在 fsm.rs 格式化函数中移除，但 generate_dot_graph 之前需要它，现在不再需要）。

- [ ] **步骤 2：运行测试验证**

运行：`cargo test --lib fsm`
预期：所有 FSM 测试 PASS

- [ ] **步骤 3：运行完整测试**

运行：`cargo test`
预期：262+ tests PASS，无新增 warning

- [ ] **步骤 4：Commit**

```bash
git add src/fsm.rs
git commit -m "refactor(fsm): convert generate_dot_graph to ReportWriter, remove 6 unwrap calls"
```

---

### 任务 2：修复 entry_signal.rs fan_in().unwrap() panic

**文件：**
- 修改：`src/entry_signal.rs:192`

- [ ] **步骤 1：修复 fan_in unwrap**

将 `dep_graph.fan_in(canonical).unwrap()` 改为安全默认值：

```rust
// Before:
let fan_in_edges = dep_graph.fan_in(canonical).unwrap();

// After:
let fan_in_edges = dep_graph.fan_in(canonical).cloned().unwrap_or_default();
```

注意：`fan_in()` 返回 `Option<&Vec<DepEdge>>`，需要 `.cloned()` 才能对 `Vec<DepEdge>` 使用 `unwrap_or_default()`。后续代码使用 `fan_in_edges.iter()` 和 `fan_in_edges.len()`，对空 Vec 同样有效。

- [ ] **步骤 2：运行测试验证**

运行：`cargo test`
预期：所有测试 PASS

- [ ] **步骤 3：Commit**

```bash
git add src/entry_signal.rs
git commit -m "fix(entry_signal): replace fan_in().unwrap() with unwrap_or_default() to prevent panic"
```

---

### 任务 3：删除 anyhow 依赖

**文件：**
- 修改：`Cargo.toml:31`
- 修改：`src/server/mod.rs:613`

- [ ] **步骤 1：修改 main() 函数签名**

在 `src/server/mod.rs:613`，将 `anyhow::Result<()>` 替换为直接错误处理：

```rust
// Before:
async fn main() -> anyhow::Result<()> {

// After:
async fn main() -> Result<(), Box<dyn std::error::Error>> {
```

整文件中 anyhow 仅此一处使用，无需其他改动。

- [ ] **步骤 2：删除 Cargo.toml 中的 anyhow 行**

在 `Cargo.toml` 中删除：
```
anyhow = "1.0"
```

- [ ] **步骤 3：运行编译验证**

运行：`cargo build`
预期：BUILD SUCCESS，无 anyhow 相关错误

- [ ] **步骤 4：运行测试验证**

运行：`cargo test`
预期：所有测试 PASS

- [ ] **步骤 5：Commit**

```bash
git add Cargo.toml src/server/mod.rs
git commit -m "chore: remove anyhow dependency, use Box<dyn Error> in main()"
```

---

### 任务 4：Server handler 用 WaveAnalyzerError 替代 format!() 错误字符串

**文件：**
- 修改：`src/server/waveform_tools.rs`
- 修改：`src/server/cdc_tools.rs`
- 修改：`src/server/protocol_tools.rs`
- 修改：`src/server/analysis_tools.rs`
- 修改：`src/server/report_tools.rs`

每个文件需添加 import：`use wave_analyzer_mcp::WaveAnalyzerError;`

**替换模式：**

| 原 format! | 替换为 WaveAnalyzerError 变体 |
|---|---|
| `format!("Waveform not found: {}", id)` | `WaveAnalyzerError::WaveformNotLoaded { id: id.clone() }` |
| `format!("Signal '{}' not found", path)` | `WaveAnalyzerError::SignalNotFound { path: path.clone() }` |
| `format!("File not found: {}", path)` | `WaveAnalyzerError::FileError { path: path.clone(), message: "not found".into() }` |
| `format!("Dependency graph not found: {}", id)` | `WaveAnalyzerError::DepsError { message: format!("Dependency graph not found: {}", id) }` |
| `.map_err(|e| McpError::invalid_params(e, None))` (e 是 WaveAnalyzerError) | `.map_err(|e| McpError::invalid_params(e, None))` — **无需改动**，Cow 转换已存在 |
| 其他 `format!(...)` 字符串 | `WaveAnalyzerError::InvalidArgument { message: ... }` 或 `WaveAnalyzerError::Other(...)` |

- [ ] **步骤 1：修改 waveform_tools.rs**

添加 import：
```rust
use wave_analyzer_mcp::WaveAnalyzerError;
```

逐一替换 `format!("Waveform not found: {}", args.waveform_id)` → `WaveAnalyzerError::WaveformNotLoaded { id: args.waveform_id.clone() }`。
逐一替换 `format!("Signal not found: {}", args.signal_path)` → `WaveAnalyzerError::SignalNotFound { path: args.signal_path.clone() }`。
逐一替换 `format!("File not found: {}", args.file_path)` → `WaveAnalyzerError::FileError { path: args.file_path.clone(), message: "not found".into() }`。
其余 `format!(...)` 字符串 → `WaveAnalyzerError::InvalidArgument { message: ... }`。

`.map_err(|e| McpError::invalid_params(e, None))` 中的 `e` 已是 `WaveAnalyzerError`（从库函数返回），Cow 转换自动工作，无需改动。

- [ ] **步骤 2：修改 cdc_tools.rs**

添加 import：
```rust
use wave_analyzer_mcp::WaveAnalyzerError;
```

替换 `format!("Waveform not found: {}", args.waveform_id)` → `WaveAnalyzerError::WaveformNotLoaded { id: args.waveform_id.clone() }`。
替换 `format!("Dependency graph not found: {}", deps_id)` → `WaveAnalyzerError::DepsError { message: format!("Dependency graph not found: {}", deps_id) }`。

- [ ] **步骤 3：修改 protocol_tools.rs**

添加 import：
```rust
use wave_analyzer_mcp::WaveAnalyzerError;
```

替换 `format!("Waveform not found: {}", args.waveform_id)` → `WaveAnalyzerError::WaveformNotLoaded { id: args.waveform_id.clone() }`。

- [ ] **步骤 4：修改 analysis_tools.rs**

添加 import：
```rust
use wave_analyzer_mcp::WaveAnalyzerError;
```

替换 `format!("Waveform not found: {}", args.waveform_id)` → `WaveAnalyzerError::WaveformNotLoaded { id: args.waveform_id.clone() }`。
替换 `format!("Dependency graph not found: {}", deps_id)` → `WaveAnalyzerError::DepsError { message: format!("Dependency graph not found: {}", deps_id) }`。

- [ ] **步骤 5：修改 report_tools.rs**

添加 import：
```rust
use wave_analyzer_mcp::WaveAnalyzerError;
```

替换 `format!("File not found: {}", args.file_path)` → `WaveAnalyzerError::FileError { path: args.file_path.clone(), message: "not found".into() }`。

- [ ] **步骤 6：运行编译验证**

运行：`cargo build`
预期：BUILD SUCCESS

- [ ] **步骤 7：运行测试验证**

运行：`cargo test`
预期：所有测试 PASS

- [ ] **步骤 8：Commit**

```bash
git add src/server/waveform_tools.rs src/server/cdc_tools.rs src/server/protocol_tools.rs src/server/analysis_tools.rs src/server/report_tools.rs
git commit -m "refactor(server): replace format!() error strings with WaveAnalyzerError variants in MCP handlers"
```

---

## 自检

1. **规格覆盖度：** §1 generate_dot_graph → 任务 1 ✓；§2 fan_in fix → 任务 2 ✓；§3 删除 anyhow → 任务 3 ✓；§4 server handler → 任务 4 ✓
2. **占位符扫描：** 无 TODO/TBD，所有步骤包含具体代码 ✓
3. **类型一致性：** `fan_in()` 返回 `Option<&Vec<DepEdge>>`，需 `.cloned().unwrap_or_default()` ✓；`WaveAnalyzerError` Cow 转换已存在 ✓；`Box<dyn std::error::Error>` 替代 anyhow ✓
