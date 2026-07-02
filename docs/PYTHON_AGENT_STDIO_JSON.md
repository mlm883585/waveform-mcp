# Python Agent Stdio JSON 接入指南

`wave-analyzer-cli agent --stdio-json` 提供面向 Python agent 的长驻进程协议。协议使用 NDJSON：每行一个 JSON 请求，每行一个 JSON 响应。

## 协议约定

| 项目 | 约定 |
|---|---|
| 启动命令 | `wave-analyzer-cli agent --stdio-json` |
| 输入 | `stdin`，一行一个 JSON request |
| 输出 | `stdout`，一行一个 JSON response |
| 日志 | `stderr` |
| 会话 | 同一进程内复用已打开 waveform/deps/assertion/spec |

请求：

```json
{"id":"1","method":"open_waveform","params":{"file_path":"dump.vcd","alias":"w"}}
```

成功响应：

```json
{"id":"1","ok":true,"method":"open_waveform","data":{"waveform_id":"w"},"summary":"Waveform opened"}
```

失败响应：

```json
{"id":"1","ok":false,"method":"open_waveform","error":{"code":"FILE_NOT_FOUND","message":"File not found: dump.vcd","recoverable":true}}
```

## Python 示例

```python
import json
import subprocess


class WaveAnalyzerAgent:
    def __init__(self, exe_path: str = "wave-analyzer-cli.exe"):
        self.proc = subprocess.Popen(
            [exe_path, "agent", "--stdio-json"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.next_id = 1

    def call(self, method: str, **params):
        request_id = str(self.next_id)
        self.next_id += 1
        request = {"id": request_id, "method": method, "params": params}
        self.proc.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
        self.proc.stdin.flush()

        line = self.proc.stdout.readline()
        if not line:
            raise RuntimeError("wave-analyzer agent exited without response")
        response = json.loads(line)
        if not response.get("ok"):
            error = response.get("error", {})
            raise RuntimeError(f"{error.get('code')}: {error.get('message')}")
        return response["data"]

    def close(self):
        try:
            self.call("shutdown")
        finally:
            self.proc.wait(timeout=5)


agent = WaveAnalyzerAgent(r"D:\tools\wave-analyzer-cli.exe")
try:
    print(agent.call("health"))
    print(agent.call("check_env"))
    agent.call("open_waveform", file_path=r"D:\sim\dump.vcd", alias="w")
    signals = agent.call("list_signals", waveform_id="w", limit=20)
    print(signals["signals"])
finally:
    agent.close()
```

## 已支持方法

| 方法 | 说明 |
|---|---|
| `health` | 返回协议版本、exe 版本和可用方法 |
| `list_methods` | 返回方法参数摘要 |
| `check_env` | 返回结构化环境检查和原始诊断文本 |
| `reset_session` | 清空当前会话状态 |
| `shutdown` | 正常退出 agent 进程 |
| `extract_deps` | 从 RTL 生成 `deps.yaml`，执行前检查输入路径和 top module |
| `analyze_run` | 执行端到端失败分析 |
| `open_waveform` / `close_waveform` | 打开或关闭波形 |
| `list_signals` | 返回信号数组 |
| `read_signal` | 返回指定时间索引下的信号值数组 |
| `find_signal_events` | 返回指定信号变化事件数组 |
| `find_conditional_events` | 返回条件命中事件数组 |

## 错误码

| 错误码 | 含义 |
|---|---|
| `FILE_NOT_FOUND` | 输入文件或目录不存在 |
| `INVALID_ARGUMENT` | 请求 JSON、方法名或参数非法 |
| `WAVEFORM_NOT_FOUND` | 当前会话中没有指定 waveform |
| `DEPS_NOT_FOUND` | 当前会话中没有指定 deps graph |
| `ENV_MISSING_DEPENDENCY` | 缺少 sidecar、iverilog、VC++ Runtime 等环境依赖 |
| `TOOL_FAILED` | 工具执行失败 |
| `INTERNAL_ERROR` | 预留内部错误 |
