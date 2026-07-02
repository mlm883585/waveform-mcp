# Vivado + ModelSim 仿真脚本设计

## 目标

Win10 专业版内网环境下的仿真执行脚本，生成可供 `wave-analyzer-mcp` 分析的日志与波形工件。

## 文件清单

| 文件 | 作用 |
|---|---|
| `run_sim_modelsim.ps1` | PowerShell 总控脚本（7 步流程） |
| `modelsim_run.tcl` | ModelSim Tcl 编译/仿真/波形导出脚本 |
| `vivado_export.tcl` | Vivado Tcl 导出 compile order + simlib |
| `sim_config_template.yaml` | 配置模板 |

## 使用方式

1. 复制 `sim_config_template.yaml` 到项目目录，修改为 `sim_config.yaml`
2. 编写 `filelist.f` 列出所有 RTL/TB 源文件
3. 运行 `powershell -File run_sim_modelsim.ps1 -ConfigPath .\sim_config.yaml`

## 输出工件

| 工件 | 位置 | 说明 |
|---|---|---|
| `transcript.log` | `sim_output/` | ModelSim 断言和仿真日志 |
| `dump.vcd` | `sim_output/` | VCD 波形文件（wellen 可直接读取） |
| `run_summary.json` | `sim_output/` | 仿真结果摘要（AI/CLI 统一入口） |
| `sim_work/` | 项目根目录 | ModelSim 工作库 |

## run_summary.json 格式

```json
{
  "status": "assertion_failed",
  "project_name": "beam_project",
  "top_module": "tb_top",
  "compile_ok": true,
  "elab_ok": true,
  "simulation_ok": true,
  "assertion_fail_count": 2,
  "warning_count": 0,
  "error_count": 2,
  "wave_file": "sim_output/dump.vcd",
  "wave_format": "vcd",
  "transcript_file": "sim_output/transcript.log",
  "simulator": "modelsim",
  "finished_at": "2026-05-26T10:30:00"
}
```

状态值：`compile_failed` | `elab_failed` | `simulation_failed` | `assertion_failed` | `passed`

## 与 wave-analyzer-mcp 集成

仿真完成后，使用 CLI 进行分析：

```bash
wave-analyzer-cli open_waveform sim_output/dump.vcd --alias mywave -- \
  load_dependencies specs/deps.yaml --alias mydeps -- \
  load_assertion_log sim_output/transcript.log --severity-filter Error,Failure -- \
  batch_trace_root_cause mywave mydeps myassertions --spec-id myspec
```