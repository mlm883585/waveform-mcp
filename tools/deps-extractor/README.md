# deps-extractor -- RTL 依赖图自动提取工具

从 RTL 设计源码自动提取信号依赖图，输出符合 `DEPS_FORMAT.md` 规范的 `deps.yaml`。

## 架构

```
RTL 源码 ──► Vivado Tcl / Pyverilog ──► deps_raw.json ──► deps_converter.py ──► deps.yaml
                                              │                                    │
                                              │         annotations.yaml ──────────┘
                                              │        （手工标注覆盖）
                                              ▼
                                       增量变更报告（--diff）
```

## 快速开始

### 方式一：Vivado Tcl（推荐，P0）

```bash
# 在 Vivado Tcl Console 中
cd path/to/wave-analyzer-mcp/tools/deps-extractor
source extract_deps.tcl
extract_deps -top beam_ctrl -output deps_raw.json -depth 2

# 或使用批处理
vivado -mode batch -source extract_deps_batch.tcl -tclargs \
  -project project.xpr -top beam_ctrl -output deps_raw.json
```

### 方式二：Python 后处理

```bash
# deps_raw.json → deps.yaml
python deps_converter.py deps_raw.json -o deps.yaml

# 合并手工标注
python deps_converter.py deps_raw.json -o deps.yaml --annotate annotations.yaml

# 增量变更报告
python deps_converter.py new_deps_raw.json -o deps_new.yaml --diff deps.yaml
```

## 文件说明

| 文件 | 说明 |
|------|------|
| `extract_deps.tcl` | Vivado Tcl 主脚本，从 elaborated design 提取依赖 |
| `extract_deps_batch.tcl` | Vivado 批处理入口（`vivado -mode batch -source`） |
| `deps_converter.py` | Python 后处理：deps_raw.json → deps.yaml |
| `annotations_template.yaml` | 手工标注模板（CDC 边界、黑盒、延迟覆盖等） |
| `examples/sample_deps_raw.json` | simple_reg 场景的中间格式示例 |
| `examples/sample_output.yaml` | 对应的最终 deps.yaml 输出 |

## 提取内容

| 提取项 | 说明 | 推断类型 |
|--------|------|----------|
| 顶层端口 | IN/OUT 端口 | boundary (input_port) |
| 模块实例 | get_cells 获取 | hierarchical 信息 |
| 网表连接 | get_nets driver→load | combinational / sequential / control |
| 触发器 | FDRE/LDCE 等 | sequential (D→Q), control (CE/RST) |
| BRAM | RAMB36E1 等 | memory (ADDR→DOUT), control (EN) |
| 时钟网络 | TYPE==CLOCK | clock_aliases |

## 依赖类型映射

| Tcl 推断来源 | deps.yaml type | latency_cycles |
|-------------|----------------|----------------|
| FF D→Q (`inferred_by: "FF"`) | sequential | 1 |
| FF CE/EN pin (`inferred_by: "FF_CE"`) | control | 0 |
| FF RST pin (`inferred_by: "FF_RST"`) | control | 0 |
| BRAM ADDR→DOUT (`inferred_by: "BRAM"`) | memory | 2 |
| BRAM EN pin (`inferred_by: "BRAM_EN"`) | control | 0 |
| 纯 wire 连接 (`inferred_by: "NET"`) | combinational | 0 |

## annotations.yaml 标注

当自动提取无法覆盖某些语义时，使用 `annotations.yaml` 补充：

```yaml
signal_overrides:     # 正则替换 Vivado 路径 → canonical 名
cdc_boundaries:       # CDC 边界标注
blackbox_modules:     # 不展开的 IP 黑盒
latency_overrides:    # 特殊延迟值覆盖
constants:            # 固定配置信号
```

## 输出格式

严格遵循 `DEPS_FORMAT.md` 规范，可直接被 `wave-analyzer-mcp` 的 `load_dependencies` 工具加载。

## 实施阶段

| 阶段 | 状态 | 说明 |
|------|------|------|
| M1: Vivado Tcl 原型 | ✅ 完成 | extract_deps.tcl 可运行，包含版本检查、时钟提取兼容、层级过滤、FF/BRAM 识别 |
| M2: Python 后处理 | ✅ 完成 | deps_converter.py 严格对齐 DEPS_FORMAT.md，sample_output.yaml 验证通过 |
| M3: 真实设计验证 | 待验证 | 在相控阵子模块上运行，验证 generate/BRAM/多通道 |
| M4: Pyverilog 备选 | ✅ 完成 | extract_deps_pyverilog.py 可从 Verilog 源码提取依赖，输出 deps_raw.json |
| M5: annotations 扩展 | ✅ 完成 | annotations_template.yaml 可用 |

## Pyverilog 方案

`extract_deps_pyverilog.py` 是纯 Python 方案，无需 Vivado：

```bash
# 安装依赖
pip install pyverilog

# 从 Verilog 源码提取
python extract_deps_pyverilog.py rtl/top.v -t top_module -o deps_raw.json

# 转换为 deps.yaml
python deps_converter.py deps_raw.json -o deps.yaml
```

**适用场景**：快速预览、CI/CD 集成、小型 Verilog-2001 设计。 

**局限**：不支持 SystemVerilog 高级特性（interface/package）、generate 展开能力有限、无 BRAM 推断。

## Sidecar EXE 发布

如果最终用户环境没有 Python，推荐把提取器打包成 sidecar：

```bash
cd tools/deps-extractor
python -m pip install pyinstaller -r requirements.txt
python build_sidecar.py
```

产物：

```text
tools/deps-extractor/dist/wave-analyzer-deps-extractor.exe
```

两种交付方式：

1. sidecar 交付：将该文件随 `wave-analyzer-cli.exe` 一起发布。
2. 单 exe 交付：先生成这个 sidecar，再执行 `cargo build --release`。
   `build.rs` 会自动把 `tools/deps-extractor/dist/wave-analyzer-deps-extractor.exe`
   嵌入 `wave-analyzer-cli.exe`，运行时解包到临时目录执行。
   Windows 维护构建也可以直接运行 `.\scripts\build-wave-analyzer-cli-single-exe.ps1`。

单 exe 交付时，最终只需要分发 `wave-analyzer-cli.exe`。

sidecar / 单 exe 两种模式都仅要求 `iverilog` 已加入系统 `PATH`。

## Update Notes

- `extract_deps.tcl` now limits hierarchy and net scans to selected cells first, instead of repeatedly traversing the full design.
- `extract_deps_batch.tcl` now forwards throttling options such as `-max_cells`, `-max_nets`, and `-enable_global_clock_fallback`.
- Vivado batch extraction now emits stage timing logs to help diagnose slow designs.
- `deps_converter.py` now processes `constants` annotations (generates `boundary_kind: "constant"` entries).
- `deps_converter.py` `_build_signal_aliases()` now actively reconciles Vivado paths with canonical names (was previously a no-op).

## Environment Variables

| 变量 | 说明 |
|------|------|
| `DEPS_EXTRACTOR_PATH` | deps-extractor 脚本目录路径。当自动路径推导失败时（如独立安装 wave-analyzer-tools），设置此变量指定 deps-extractor 位置。例如：`DEPS_EXTRACTOR_PATH=/path/to/wave-analyzer-mcp/tools/deps-extractor` |
