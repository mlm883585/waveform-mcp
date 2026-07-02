# 代码质量清理设计规格

## 背景

wave-analyzer-mcp v0.5.0 已完成错误处理重构（WaveAnalyzerError enum、ReportWriter helper、resolve_signal_with_width），但遗留 4 处代码质量问题需清理。

---

## §1：generate_dot_graph 转 ReportWriter

### 问题
`src/fsm.rs:388-423` 的 `generate_dot_graph` 函数有 6 处 `writeln!(dot, ...).unwrap()` 调用，是唯一未迁移到 ReportWriter 的格式化函数。

### 改动
- 将 `generate_dot_graph` 内部 `String` → `ReportWriter`
- `writeln!(dot, ...).unwrap()` → `report_writeln!(dot, ...)`
- `writeln!(dot, "}}").unwrap()` → `report_writeln!(dot, "}}")`
- 返回 `out.finish()`

### 验证
- FSM 测试输出不变（DOT graph 内容完全一致）

---

## §2：entry_signal.rs fan_in().unwrap() 修复

### 问题
`src/entry_signal.rs:192` — `dep_graph.fan_in(canonical).unwrap()` 在 `is_output_node()` 为 true 时调用，但 `fan_in()` 返回 `Option`，`is_output_node` 不保证 `fan_in` 有值。

### 改动
- `fan_in(canonical).unwrap()` → `fan_in(canonical).unwrap_or_default()` 或 `fan_in(canonical).unwrap_or_else(|| Vec::new())`
- 语义：当 deps 图中没有 fan-in 边时，空列表是正确的默认行为

### 验证
- `cargo test` 全部通过
- 无运行时 panic 风险

---

## §3：删除 anyhow 依赖

### 问题
`anyhow = "1.0"` 仅在 `src/server/mod.rs:613` 的 `main()` 函数中使用。全库已迁移到 `WaveAnalyzerError`，anyhow 不再需要。

### 改动
- Cargo.toml 删除 `anyhow = "1.0"` 行
- `main()` 函数错误处理改为 `Result<(), Box<dyn std::error::Error>>` 或直接 `.expect()` / `eprintln!` 模式

### 验证
- `cargo build` 成功
- `cargo test` 全部通过

---

## §4：Server handler 用 WaveAnalyzerError 替代 format!() 错误字符串

### 问题
约 25 处 `McpError::invalid_params(format!("Waveform not found: {}", id), None)` 和类似模式。应使用 `WaveAnalyzerError` 的具体变体，利用已有的 `From<WaveAnalyzerError> for Cow<'static, str>` 自动转换。

### 改动模式
```rust
// Before:
McpError::invalid_params(format!("Waveform not found: {}", wf_id), None)

// After:
McpError::invalid_params(WaveAnalyzerError::WaveformNotLoaded { id: wf_id.clone() }, None)
```

涉及变体映射：
- `format!("Waveform not found: {}", id)` → `WaveAnalyzerError::WaveformNotLoaded { id }`
- `format!("Signal '{}' not found", path)` → `WaveAnalyzerError::SignalNotFound { path }`
- `format!("Invalid argument: {}", msg)` → `WaveAnalyzerError::InvalidArgument { message }`
- 其他通用 `format!(...)` → `WaveAnalyzerError::Other(msg)`

### 验证
- `cargo build` 成功（Cow 转换已存在）
- MCP 工具行为不变（错误消息内容一致，thiserror #[error] 格式匹配原 format! 内容）

---

## 实施顺序

| 步骤 | 范围 | 改动文件数 |
|---|---|---|
| §1 ReportWriter | 1 文件 | src/fsm.rs |
| §2 fan_in 修复 | 1 文件 | src/entry_signal.rs |
| §3 删除 anyhow | 2 文件 | Cargo.toml + src/server/mod.rs |
| §4 Server handler | 1 文件 | src/server/mod.rs |

每步完成后独立 commit。
