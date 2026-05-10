# Relay

轻量级 AI 代理，运行在你的终端里。 / Lightweight AI agent that runs in your terminal.

连接 DeepSeek（或任意兼容 OpenAI 接口的 API），在本地执行工具——shell 命令、文件读写、搜索等。 / Connects to DeepSeek (or any OpenAI-compatible API) and executes tools locally — shell commands, file operations, search, and more.

---

## 目录 / Table of Contents

- [宗旨 / Philosophy](#宗旨--philosophy)
- [快速开始 / Quick Start](#快速开始--quick-start)
- [首次设置 / First-Time Setup](#首次设置--first-time-setup)
- [工作原理 / How It Works](#工作原理--how-it-works)
- [配置 / Configuration](#配置--configuration)
- [文件说明 / Files](#文件说明--files)
- [依赖 / Requirements](#依赖--requirements)
- [许可证 / License](#许可证--license)

---

## 宗旨 / Philosophy

**简单。顺手。比 "it just works" 稍微可靠一点。** / **Simple. Handy. Reliable enough.**

- **简单 / Simple** — 零外部依赖，一个配置文件，一条命令启动。 / Zero dependencies by default, one config file, one command to start.
- **顺手 / Handy** — 扔到哪都能跑，自动识别 OS 和 Shell，首次启动引导配置。 / Drop it anywhere, it detects your OS and shell, guides you through first-time setup.
- **可靠 / Reliable** — 自动重试退避、上下文窗口裁剪、异常优雅恢复。不会随便崩。 / Retry with backoff, context-window pruning, graceful error recovery — it won't crash on you.

---

## 快速开始 / Quick Start

```bash
python chat.py

# Windows 下也可以 / Or on Windows:
chat.cmd
```

首次启动会自动引导你配置 API 密钥和其他设置。 / First launch will walk you through API key setup interactively.

### 启动选项 / Options

```bash
python chat.py --model deepseek-reasoner
```

### 命令 / Commands

| 命令 / Command | 说明 / Description |
|----------------|-------------------|
| `/exit` | 退出 / Quit |
| `/clear` | 清空对话 / Clear conversation |
| `/model <name>` | 动态切换模型 / Switch model on the fly |
| `/tools` | 列出可用工具 / List available tools |
| `/tokens` | 显示预估 token 用量 / Show estimated token usage |

---

## 首次设置 / First-Time Setup

首次运行 relay 时，会自动检查 `~/.relay/settings.json`。如果不存在，会进入交互式引导：

1. 扫描你电脑上已有的配置源（`~/.deepseek/config.toml`、`~/.claude/settings.json`）
2. 让你选择从哪个导入，或手动输入
3. 询问 Enter 行为偏好

配置写入 `~/.relay/settings.json` 后即可开始使用。

---

## 工作原理 / How It Works

1. **环境自适应 / Environment detection** — relay 自动检测运行环境（Windows/Linux、cmd/powershell/bash/zsh），生成对应的系统提示词。 / Detects your OS and shell, builds a tailored system prompt.
2. **流式响应 / Streaming** — 你输入消息，relay 从 API 流式获取回复。 / You type a message, relay streams the response from the API.
3. **工具调用 / Tool calling** — 模型可以请求调用工具（shell、read、write、glob、grep），relay 在本地执行并将结果返回给模型。 / The model can request tool calls — relay executes them locally and feeds results back.
4. **多轮交互 / Multi-turn** — 单个轮次内支持多轮工具调用，达到上限后停止。 / Multiple tool rounds per turn, up to the configured limit.
5. **上下文管理 / Context pruning** — 接近 token 上限时自动裁剪早期对话，保持上下文在窗口内。 / Automatically prunes early messages when approaching the model's token limit.

---

## 配置 / Configuration

配置存储在 `~/.relay/settings.json` / Settings live in `~/.relay/settings.json`:

```json
{
    "api_key": "sk-...",
    "base_url": "https://api.deepseek.com",
    "model": "deepseek-chat",
    "enter_sends": true
}
```

| 字段 / Field | 说明 / Description |
|-------------|-------------------|
| `api_key` | API 密钥 / API key |
| `base_url` | API 端点 / API base URL |
| `model` | 模型名称 / Model name |
| `enter_sends` | Enter 发送（true）或换行（false）/ Enter sends (true) or inserts newline (false) |

环境变量可覆盖文件设置 / Environment variables override file settings:

- `DEEPSEEK_API_KEY`
- `DEEPSEEK_BASE_URL`
- `DEEPSEEK_MODEL`

向后兼容 `~/.deepseek/config.toml` / Backward compatible with `~/.deepseek/config.toml`.

---

## 文件说明 / Files

| 文件 / File | 作用 / What it does |
|-------------|-------------------|
| `chat.py` | 入口 — REPL 交互循环 / Entry point — REPL loop |
| `relay_config.py` | `~/.relay/` 文件夹管理 & 首次配置引导 / Config folder management & first-time setup |
| `config.py` | 配置加载（settings.json → 环境变量 → deepseek toml）/ Configuration loader |
| `client.py` | API 客户端（自动重试 + 流式）/ API client with retry & streaming |
| `session.py` | 对话历史 & 上下文裁剪 / Conversation history & context pruning |
| `tools.py` | 工具定义与执行 / Tool definitions and execution |
| `env_detect.py` | OS / Shell 检测 / OS & shell detection |
| `exceptions.py` | 异常类型 / Error types |
| `chat.cmd` | Windows 启动器 / Windows launcher |
| `test_integration.py` | 集成测试 / Integration tests |

---

## 依赖 / Requirements

- **Python 3.10+**
- [DeepSeek](https://platform.deepseek.com) 或其他兼容 OpenAI 接口的 API 密钥 / An API key from DeepSeek or any OpenAI-compatible provider
- 可选 / Optional: `pip install prompt_toolkit` 开启多行输入支持 / for multi-line input support

---

## 许可证 / License

MIT
