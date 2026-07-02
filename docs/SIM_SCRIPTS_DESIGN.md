# Vivado + ModelSim 仿真脚本设计

## 1. 目标

本文档定义 Win10 专业版内网环境下的仿真执行方案，服务于以下目标：

1. 让用户能够稳定运行 Vivado 2018.3 + ModelSim 20.1.1.720 联合流程。
2. 生成可供 `wave-analyzer-mcp` / `wave-analyzer-cli` 分析的日志与波形工件。
3. 为 AI 提供确定性、可重复、可审计的仿真入口。

---

## 2. 工具职责

| 工具 | 职责 |
|---|---|
| Vivado 2018.3 | 工程文件管理、IP 输出、第三方仿真库准备、可选导出 compile order |
| ModelSim 20.1.1.720 | 编译 RTL/TB、执行仿真、执行断言、生成 Transcript、导出波形 |
| PowerShell | 总控脚本、环境准备、文件组织、结果汇总 |
| Tcl | 传给 ModelSim 的编译/仿真命令序列 |

### 2.1 推荐工作方式

主流工程实践下，Vivado 不直接替代 ModelSim，而是为第三方仿真器提供：

1. 仿真库准备。
2. RTL/IP 文件列表。
3. 工程一致的参数/宏/包含目录。

ModelSim 负责实际仿真。

---

## 3. 输出工件

仿真脚本至少应稳定产生以下工件：

| 工件 | 作用 |
|---|---|
| `sim_output/transcript.log` | 断言和仿真日志 |
| `sim_output/dump.vcd` 或 `sim_output/dump.wlf` | 波形证据 |
| `sim_output/run_summary.json` | 仿真结果摘要，供 AI/CLI 消费 |
| `sim_work/` | ModelSim 工作目录 |

建议将 `run_summary.json` 作为后续 AI 编排的统一入口，而不是让 AI 直接解析大量命令行输出。

---

## 4. 配置文件建议

继续使用 `sim_config.yaml`，但字段需明确区分 Vivado 与 ModelSim。

```yaml
project_name: "beam_project"
project_root: "D:/projects/beam_project"

vivado:
  enabled: true
  vivado_bin: "D:/Xilinx/Vivado/2018.3/bin/vivado.bat"
  project_file: "beam_project.xpr"
  compile_simlib_dir: "D:/eda_libs/vivado_2018_3_modelsim"
  export_compile_order: true

modelsim:
  modelsim_bin: "D:/MentorGraphics/ModelSim/win64"
  work_lib: "sim_work/work"
  top_module: "tb_top"
  vlog_flags:
    - "-sv"
    - "+acc"
    - "+define+SIM"
  vsim_flags:
    - "-c"
    - "-t"
    - "1ps"
    - "-assertdebug"

compile_sources:
  filelist: "sim/filelist.f"
  include_dirs:
    - "rtl/include"
    - "tb/include"
  macros:
    - "SIM"

simulation:
  mode: "run_all"           # run_all | fixed_time
  run_time: null            # mode=fixed_time 时有效
  stop_on_error: false
  transcript_file: "sim_output/transcript.log"
  summary_file: "sim_output/run_summary.json"

wave_dump:
  format: "vcd"             # vcd | wlf
  file: "sim_output/dump.vcd"
  recursive_scopes:
    - "/tb_top/dut/*"
  critical_signals:
    - "/tb_top/dut/beam_data_o"
    - "/tb_top/dut/coeff_valid"
```

---

## 5. 目录组织建议

### 5.1 用户项目侧

```text
my_fpga_project/
├── rtl/
├── tb/
├── sim/
│   ├── filelist.f
│   ├── modelsim_run.tcl
│   └── vivado_export.tcl
├── specs/
│   ├── design_spec.yaml
│   └── deps.yaml
├── sim_output/
│   ├── transcript.log
│   ├── dump.vcd
│   └── run_summary.json
└── sim_config.yaml
```

### 5.2 `wave-analyzer-mcp` 项目侧

```text
wave-analyzer-mcp/
└── docs/
    └── SIM_SCRIPTS_DESIGN.md
```

脚本模板可以由用户项目引用，也可以后续在 CLI 中提供生成器。

---

## 6. 执行流程

### 6.1 标准流程

| 步骤 | 执行动作 | 输出 |
|---|---|---|
| 1 | 检查工具路径与环境变量 | 可执行环境 |
| 2 | 如启用 Vivado，则检查仿真库和导出文件列表 | `filelist.f` / 仿真库映射 |
| 3 | 创建/清理 `sim_work` | 干净工作目录 |
| 4 | 调用 `vlog` 编译 RTL/TB | 编译日志 |
| 5 | 调用 `vsim` 运行仿真 | Transcript、波形 |
| 6 | 汇总生成 `run_summary.json` | 统一摘要 |
| 7 | 交由 MCP/CLI/AI 做后分析 | 分析报告 |

### 6.2 仿真状态归类

| 状态 | 条件 |
|---|---|
| `compile_failed` | `vlog` 非零退出 |
| `elab_failed` | `vsim` 启动失败 |
| `simulation_failed` | 仿真中异常退出 |
| `assertion_failed` | 仿真跑完，但存在 error 级断言失败 |
| `passed` | 仿真完成且无关键失败 |

这比简单扫描 console 文本更适合作为自动化闭环入口。

---

## 7. Transcript 设计约束

### 7.1 Transcript 的职责

Transcript 只负责提供：

1. 断言名称。
2. 严重级别。
3. 时间值与单位。
4. scope。
5. 可选源文件和行号。

### 7.2 Transcript 不承担的职责

Transcript 不应被设计为：

1. 直接提供失败信号路径。
2. 直接决定 BFS 入口信号。
3. 替代 `design_spec.yaml.assertions[].observe_signals`。

这条必须和 `BFS_ENGINE_DESIGN.md` 保持一致。

---

## 8. 波形导出策略

### 8.1 推荐格式

| 格式 | 建议 |
|---|---|
| VCD | 第一优先，兼容性最好，便于 `wellen` 直接分析 |
| WLF | 本地查看方便，可保留，但分析前需要导出/转换 |

### 8.2 推荐策略

| 场景 | 建议 |
|---|---|
| 首次问题定位 | dump DUT 关键层级全部内部信号 |
| 日常快速回归 | 只 dump 关键路径和断言相关信号 |
| BFS 深追场景 | 保证依赖图涉及的内部信号都在 dump 范围内 |

### 8.3 `+acc` 策略

不建议在所有日常仿真中一律全量 `+acc`，建议分层：

| 模式 | 配置 |
|---|---|
| 快速回归 | 低可见性，少量波形 |
| 调试回归 | `+acc` + 关键层级 dump |
| 深度追溯 | `+acc` + BFS 路径全覆盖 |

---

## 9. Vivado 集成建议

### 9.1 推荐使用方式

Vivado 在本工作流中适合作为：

1. IP 与库的来源。
2. 工程 compile order 的来源。
3. `compile_simlib` 的执行入口。

### 9.2 不建议使用方式

1. 不建议让 Vivado XSim 承担主功能仿真闭环。
2. 不建议同时维护两套独立 TB/SVA 流程。

### 9.3 推荐前置动作

用户首次部署时建议执行一次：

1. Vivado `compile_simlib` 为 ModelSim 生成对应仿真库。
2. 固化输出目录，并在 `sim_config.yaml` 中引用。

---

## 10. PowerShell 脚本设计要求

### 10.1 脚本行为要求

| 要求 | 说明 |
|---|---|
| 幂等 | 多次运行不应破坏工程 |
| 可清理 | 支持 `-CleanWork` |
| 可跳过编译 | 支持 `-SkipCompile` |
| 可只编译不跑 | 支持 `-SkipSimulate` |
| 结果结构化 | 输出 `run_summary.json` |

### 10.2 配置解析要求

内网环境下不应依赖在线安装 PowerShell YAML 模块。

推荐顺序：

1. 使用脚本自带解析方式或后续由 Rust CLI 解析配置。
2. 若必须使用 PowerShell YAML 解析，则作为可选增强，不作为必需依赖。

---

## 11. ModelSim Tcl 设计要求

### 11.1 Tcl 职责

Tcl 只负责：

1. 设置 Transcript。
2. 加载波形导出规则。
3. 运行仿真。
4. 输出断言统计。
5. 退出并刷新波形。

### 11.2 推荐行为

| 项目 | 建议 |
|---|---|
| 运行方式 | TB 有 `$finish` 时优先 `run -all` |
| 波形导出 | 仿真一开始即配置 |
| 断言策略 | 默认记录全部 error，不因单个失败立即退出 |
| 退出前动作 | 刷新波形、输出 summary |

---

## 12. 与 MCP/CLI 的衔接

### 12.1 推荐链路

```text
run_sim_modelsim.ps1
    ↓
run_summary.json + transcript.log + dump.vcd
    ↓
wave-analyzer-cli / MCP
    ├─ open_waveform
    ├─ parse_assertion_log   (计划接口)
    ├─ load_dependencies     (计划接口)
    └─ trace_root_cause      (计划接口)
```

### 12.2 文档约束

`parse_assertion_log`（现名为 `load_assertion_log`）、`load_dependencies`、`trace_root_cause` 及后续 `batch_trace_root_cause`、`export_bfs_report`、`load_run_summary` 等工具均已实现。

因此本文件各工具可正常引用为已实现功能。

---

## 13. 结果判定建议

### 13.1 `run_summary.json` 推荐字段

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

### 13.2 AI 使用方式

AI 读取该摘要后：

1. 若 `compile_ok=false`，优先审查编译问题。
2. 若 `assertion_fail_count>0`，再解析 transcript 并进入 BFS。
3. 若 `status=passed`，本轮流程结束。

### 13.3 `run_summary.json` 到 BFS 的标准编排顺序

建议将自动化链路固定为：

```text
run_summary.json
  -> 判定 status
  -> 若 assertion_failed:
       parse_assertion_log(transcript.log)
       -> 选择一个 AssertionEvent
       -> 从 spec 获取 observe_signals / fail_entry_signals
       -> 将 transcript 时间值映射到波形 time_index
       -> load_dependencies(deps.yaml)
       -> trace_root_cause(signal_path, time_index, deps_alias)
```

补充要求：

1. `run_summary.json` 不直接携带 BFS 入口信号。
2. Transcript 不直接充当失败信号来源，只提供失败事件和时间。
3. `trace_root_cause` 第一版建议只接收 `time_index`，因此脚本或上层编排必须先完成时间映射。

---

## 14. 已知限制

| 限制 | 影响 | 缓解方式 |
|---|---|---|
| 大项目全量 VCD 很大 | 生成慢、加载慢 | 关键层级 dump + 必要时保留 WLF |
| 老 Verilog 工程文件顺序敏感 | 编译易失败 | 用 Vivado 导出 compile order |
| 第三方仿真库配置复杂 | 初次部署成本高 | 首次部署固化 `compile_simlib` 结果 |
| Transcript 版本格式可能有差异 | 正则解析需适配 | 以实际 20.1.1.720 输出为准做测试 |

---

## 15. 与其他文档的一致性要求

| 文档 | 一致性要求 |
|---|---|
| `WORKFLOW_DESIGN.md` | Vivado 管工程，ModelSim 管仿真 |
| `DESIGN_SPEC_FORMAT.md` | 断言失败后的入口信号来自 spec |
| `BFS_ENGINE_DESIGN.md` | Transcript 不直接给失败信号，BFS 以 spec + deps 为准 |

如有冲突，以“仿真日志只提供失败事件，不直接定义根因入口”为准。
