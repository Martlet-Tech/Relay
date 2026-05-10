# Relay

## 目录结构 / Directory Structure

```
Relay/
├── chat.py              入口 — REPL 交互循环
│                        Entry point — REPL loop
├── relay_config.py      ~/.relay/ 文件夹管理 & 首次配置引导
│                        Config folder management & first-time setup
├── config.py            配置加载（settings.json → 环境变量 → deepseek toml）
│                        Configuration loader
├── client.py            API 客户端（自动重试 + 流式 SSE 解析）
│                        API client with retry & streaming
├── session.py           对话历史 & 上下文裁剪
│                        Conversation history & context pruning
├── tools.py             工具定义（OpenAI function-calling schema）与执行
│                        Tool definitions & execution
├── env_detect.py        OS / Shell 检测
│                        OS & shell detection
├── exceptions.py        异常类型（ConfigError, APIError, AuthError, etc.）
│                        Error types
├── chat.cmd             Windows 启动器
│                        Windows launcher
├── test_integration.py  集成测试（13 tests: config, env, tools, session, API）
│                        Integration tests
├── README.md            中英双语文档
│                        Bilingual documentation
├── CLAUDE.md            项目结构说明（本文件）
│                        Project structure guide
├── LICENSE              MIT License
└── .gitignore
```

## 架构要点 / Architecture Notes

- **无外部依赖**（prompt_toolkit 可选）
- 使用标准库 `urllib.request` 调用 API，无第三方 HTTP 库
- SSE 流式解析在 `client.py` 中手工处理
- 对话上下文按字符估算 token（~4.5 chars/token），超出时从最早的 tool 消息开始裁剪
- 工具定义使用 OpenAI function-calling schema，执行器在 `tools.py` 中
- 配置源优先级：`~/.relay/settings.json` → 环境变量 → `~/.deepseek/config.toml`（向后兼容）
- 配置文件在 `relay_config.py` 中管理，首次启动时交互式引导

## 关键流程 / Key Flow

```
chat.py main()
  → relay_config.ensure_settings()  首次配置引导
  → config.load_config()            加载配置
  → env_detect.detect_environment() 检测 OS/Shell
  → Session                          创建会话
  → REPL loop:
      input → Session.add_user_message()
      → client.stream_chat_completion()  SSE 流式请求
      → 逐块处理 content / reasoning / tool_call
      → tools.execute_tool()            执行工具
      → Session.add_tool_result()       送回结果
      → 循环直到无 tool_call 或达到 max_tool_turns
```

## 配置 / Config

`~/.relay/settings.json`:

```json
{
    "api_key": "sk-...",
    "base_url": "https://api.deepseek.com",
    "model": "deepseek-chat",
    "enter_sends": true
}
```
