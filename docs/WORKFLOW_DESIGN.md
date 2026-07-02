# 波形分析系统总体设计

## 1. 文档目标

本文档定义一套适用于内网 FPGA 团队的通用验证与调试工作流，目标环境如下：

| 项目 | 约束 |
|---|---|
| 操作系统 | Windows 10 专业版 |
| 综合工具 | Vivado 2018.3 |
| 功能仿真 | ModelSim 20.1.1.720 |
| RTL 语言 | Verilog，语法兼容 Vivado 2018.3 |
| 测试方式 | Testbench + SVA 断言，不使用 cocotb |
| 代码规模 | 十几万行级 FPGA 代码库 |
| 应用方向 | 相控阵天线及相关数据通路/控制链路 |
| 部署环境 | 内网，离线运行 |

本文档同时约束 `wave-analyzer-mcp` 与 `wave-analyzer-cli` 的职责边界，作为后续 Phase 3 设计的统一基线。

---

## 2. 设计原则

参考主流 FPGA/ASIC 仿真调试实践，本方案采用以下原则：

| 原则 | 含义 | 对本项目的约束 |
|---|---|---|
| 需求先行 | 先有明确规格，再有 RTL 与验证 | 必须先编写 `design_spec.yaml` |
| 断言优先 | 先靠 TB/SVA 定义正确性，再做波形复盘 | ModelSim 是功能仿真和断言执行主引擎 |
| 波形是证据 | 波形用于复盘、定位、解释，不替代规格 | `wave-analyzer-mcp` 负责读取和分析波形 |
| 图驱动调试 | 根因定位依赖人工维护的依赖图 | BFS 基于 `deps.yaml`，不直接“猜 RTL” |
| 工具分层 | 综合、仿真、分析分层解耦 | Vivado、ModelSim、MCP/CLI 各司其职 |
| 可审计 | AI 所有结论都应可追溯到规格、断言、波形、依赖图 | 不允许仅靠自然语言给出“黑盒结论” |
| 渐进式落地 | 先覆盖关键链路，再逐步扩展 | 先覆盖 top 关键数据链路、控制链路、时钟域边界 |

---

## 3. 行业对齐后的工具分工

### 3.1 Vivado 与 ModelSim 的职责分层

| 工具 | 主职责 | 不负责的内容 |
|---|---|---|
| Vivado 2018.3 | 工程管理、IP/库准备、综合实现、导出仿真文件列表、`compile_simlib` | 不作为主功能仿真与断言平台 |
| ModelSim 20.1.1.720 | RTL/TB 编译、功能仿真、SVA 执行、Transcript、波形导出 | 不负责规格管理和根因图分析 |
| wave-analyzer-mcp | 波形读取、条件搜索、依赖图查询、BFS 根因分析、断言日志解析 | 不负责编译 RTL |
| wave-analyzer-cli | 脚本化调用入口，适合 CI/批处理 | 不替代 MCP 交互式使用 |
| AI Agent | 读取规格、生成/审查 RTL 与 TB、调度工具、解释结果 | 不直接替代仿真器与形式化证据 |

说明：

1. 上表中 `wave-analyzer-mcp` / `wave-analyzer-cli` 的“依赖图查询、BFS 根因分析、断言日志解析”属于目标职责边界。
2. 结合当前仓库实现状态，上述所有 MCP 工具（30+）均已实现，包括波形操作、依赖图加载、断言日志解析、BFS 根因追溯、批量追溯、报告导出等。
3. 依赖图加载、断言日志解析、BFS 追溯已在 Phase 3-6 完成实现，可在其他文档中正常引用。

### 3.2 推荐集成方式

1. Vivado 负责导出工程所需 RTL/IP 文件清单，并预编译仿真库到 ModelSim 可用目录。
2. ModelSim 负责实际 `vlog/vsim` 编译和执行 SVA。
3. 仿真结束后输出 `transcript.log` 与 `dump.vcd`/`dump.wlf`。
4. `wave-analyzer-mcp` 或 `wave-analyzer-cli` 负责波形复盘、条件搜索、依赖图追溯和报告生成。
5. AI 只在上述证据之上做解释和修改建议。

这符合第三方仿真器接入 FPGA 工程的主流工作方式，也更适合 Win10 内网环境。

---

## 4. 总体架构

```text
design_spec.yaml
    ↓
AI 生成/审查 RTL、TB、SVA、deps.yaml
    ↓
Vivado 导出工程文件列表 / 仿真库准备
    ↓
ModelSim 编译 + 仿真
    ↓
transcript.log + dump.vcd/wlf
    ↓
wave-analyzer-mcp / wave-analyzer-cli
    ├─ 断言日志解析
    ├─ 条件事件搜索
    ├─ 依赖图查询
    └─ BFS 根因分析
    ↓
AI 生成调试结论与修改建议
    ↓
回写 RTL / TB / deps.yaml / spec
```

> 注：上图是目标闭环架构。其中“断言日志解析、依赖图查询、BFS 根因分析”在当前仓库中仍属于 Phase 3 计划能力。

---

## 5. 核心工件

| 工件 | 作用 | 维护者 |
|---|---|---|
| `design_spec.yaml` | 设计需求、接口、行为、断言入口、调试入口信号 | 设计/验证工程师 |
| `deps.yaml` | 供 BFS 使用的信号依赖图 | 设计工程师主导，AI辅助维护 |
| `sim_config.yaml` | 仿真脚本配置 | 验证工程师 |
| `tb_top.sv` | Testbench 与 SVA 断言 | 验证工程师 / AI |
| `transcript.log` | 断言与仿真日志证据 | ModelSim 生成 |
| `dump.vcd` / `dump.wlf` | 波形证据 | ModelSim 生成 |

---

## 6. 标准工作流

### 6.1 主流程

| 阶段 | 输入 | 输出 | 成功条件 |
|---|---|---|---|
| 1. 需求建模 | 用户需求、接口约束 | `design_spec.yaml` | 规格字段完整且可验证 |
| 2. 设计实现 | spec、现有代码库 | RTL / 修改建议 | 代码可编译 |
| 3. 验证建模 | spec、RTL | TB、SVA、`deps.yaml` | 能覆盖关键行为 |
| 4. 仿真执行 | RTL、TB、sim_config | transcript、波形 | 仿真可稳定结束 |
| 5. 自动分析 | transcript、波形、deps | 通过/失败报告、BFS 树 | 失败可定位到具体链路 |
| 6. 修复闭环 | 分析报告 | RTL/TB/deps/spec 更新 | 回归通过 |

### 6.2 失败分流

| 失败类型 | 首选处理路径 |
|---|---|
| 编译失败 | AI 优先审查语法、文件顺序、库映射 |
| Elaborate/加载失败 | 检查顶层、参数、仿真库、未绑定模块 |
| 断言失败 | 先看 Transcript，再进入 BFS |
| 无断言但行为异常 | 用 `find_conditional_events` 做条件搜索，再进入 BFS |
| 波形不足 | 回到仿真脚本补充 dump 范围或 `+acc` |

---

## 7. BFS 在工作流中的位置

### 7.1 BFS 的职责

BFS 不是替代断言，而是失败后的根因缩小器：

1. 断言或条件搜索先确定“哪一个行为失败”。
2. `design_spec.yaml` 给出该行为或断言对应的入口信号。
3. `deps.yaml` 定义入口信号往上游怎么追。
4. BFS 结合波形和依赖图，把问题从“输出错了”收缩到“哪一级/哪类控制导致了错误”。

### 7.2 BFS 不负责的内容

| 不负责项 | 原因 |
|---|---|
| 自动理解全部 RTL 组合逻辑 | 成本高，且对老 Verilog 代码不稳定 |
| 自动跨时钟域推理 | 需要显式 CDC 语义，不能靠波形硬猜 |
| 直接证明设计正确 | BFS 只定位疑点，不替代断言和规格 |

---

## 8. 时间模型统一约束

这是本方案最重要的落地约束。

### 8.1 禁止使用的错误模型

以下模型在本项目中视为错误设计：

```text
dep_time_index = current_time_index - latency_cycles
```

原因：

1. 波形 `time_table` 是事件时间点，不是时钟拍号。
2. 同一个时钟周期可能对应多个 time index。
3. 某些周期如果信号不变，目标信号可能没有事件点。

### 8.2 正确模型

`deps.yaml` 中所有时延统一定义为：

| 字段 | 含义 |
|---|---|
| `clock` | 此依赖边所属参考时钟 |
| `latency_cycles` | 相对于该参考时钟边沿回退的周期数 |
| `edge` | `posedge` 或 `negedge` |

追溯步骤：

1. 先把故障时间映射到最近一次参考时钟边沿。
2. 再按 `latency_cycles` 回退 N 个时钟边沿。
3. 最后在该时刻附近读取上游信号值。

这一定义同时适用于流水线、寄存器、BRAM 读延迟等时序依赖。

---

## 9. 相控阵项目的专项约束

相控阵相关 FPGA 设计常见以下结构，文档与算法应优先支持：

| 结构 | 典型问题 | 文档建议 |
|---|---|---|
| 多通道并行数据链路 | 通道间延迟不一致、相位/幅度错位 | `deps.yaml` 为每通道保留 canonical 命名 |
| 系数装载与 LUT/BRAM 读取 | 系数生效拍数不清、地址/数据对不上 | 明确 `memory` 边与 `control` 边 |
| 波束控制状态机 | 状态切换窗口错误 | 在 spec 中定义行为与断言 |
| 反压/有效握手链路 | `valid/ready` 时序错位 | 单独建握手依赖与断言 |
| CDC/多时钟域接口 | 边界判断错误 | 当前版本只定位到 CDC 边界，不跨域自动展开 |

---

## 10. AI 介入边界

### 10.1 AI 适合做的事情

| 类型 | 说明 |
|---|---|
| 规格解释 | 读取 `design_spec.yaml` 并抽取关键行为 |
| RTL 审查 | 对照规格检查实现缺口 |
| TB/SVA 草拟 | 生成测试场景与断言初稿 |
| 波形分析编排 | 调用 MCP/CLI 查询波形、依赖图、BFS |
| 根因解释 | 基于证据总结异常链路 |

### 10.2 AI 不应直接决定的事情

| 类型 | 原因 |
|---|---|
| “通过/失败”的最终判断 | 必须由断言、条件检查或人工规则给出 |
| 跨时钟域真实因果 | 需要工程师补充语义 |
| 未建模链路的根因断言 | 缺少 `deps.yaml` 证据时只能给疑点，不能给结论 |

---

## 11. 与其他文档的关系

| 文档 | 作用 |
|---|---|
| `DESIGN_SPEC_FORMAT.md` | 定义规格文档结构 |
| `DEPS_FORMAT.md` | 定义 BFS 使用的依赖图格式 |
| `BFS_ENGINE_DESIGN.md` | 定义根因分析算法 |
| `BFS_ALGORITHM_GUIDE.md` | BFS 算法入门教程与使用指南 |
| `SIM_SCRIPTS_DESIGN.md` | 定义 Vivado/ModelSim 集成仿真脚本 |
| `INTERFACE_CONTRACTS.md` | 定义 MCP/CLI/仿真摘要的输入输出契约 |
| `MINIMAL_REFERENCE_EXAMPLE.md` | 提供最小可闭环 spec/deps/transcript/VCD/BFS 样板 |
| `DEPS_EXTRACTION_DESIGN.md` | 定义从 RTL 源码自动提取 deps.yaml 依赖图的方案 |

本文件优先定义系统边界；其余文档不得与本文件在时间模型、工具分工、失败链路上冲突。
