# Relay 软件设计文档

## 概述

Relay 是一个运行在终端中的 AI Agent，支持 OpenAI 兼容 API。提供全屏 TUI 和经典 REPL 两种交互模式。

用 Rust 重写，目标：
- 单二进制分发，无需 Python 运行时
- ratatui 实现更丰富的聊天 UI
- 内置防钻牛角尖机制
- 可切换运行模式（Auto / Confirm / Plan）
- 集成记忆系统和技能系统

---

## 仓库结构

```
Relay/
├── README.md               # 项目说明、安装、使用
├── Cargo.toml              # Rust 项目配置
├── docs/
│   └── design.md           # 本文件：软件设计文档
├── src/
│   ├── main.rs             # 入口、CLI、启动
│   ├── error.rs            # 错误类型
│   ├── config.rs           # 配置加载
│   ├── message.rs          # 消息类型与会话管理
│   ├── client.rs           # API 通信
│   ├── tools.rs            # 工具定义与执行
│   ├── env.rs              # 环境检测
│   ├── memory.rs           # 记忆系统
│   ├── skill.rs            # 技能系统
│   ├── mode.rs             # 运行模式管理
│   ├── reflect.rs          # 防钻机制
│   ├── supervisor.rs       # 监督者接口（预留）
│   ├── app.rs              # 对话编排
│   ├── setup.rs            # 首次设置向导
│   ├── ui.rs               # UI 辅助函数
│   ├── term.rs             # 终端模式
│   └── tui.rs              # 全屏 TUI
├── tools/
│   ├── install.bat         # 安装到 PATH
│   ├── uninstall.bat       # 从 PATH 移除
│   ├── update.bat          # git pull 更新
│   └── relay.cmd           # Windows Terminal 启动器
└── tests/
    └── integration.rs      # 集成测试
```

---

## 架构分层

4 层架构，依赖单向向下，同层模块互不依赖。

```
┌─────────────────────────────────────────┐
│  Layer 4  表现层                         │
│  main.rs · ui.rs · term.rs · tui.rs     │
├─────────────────────────────────────────┤
│  Layer 3  编排层                         │
│  app.rs · reflect.rs · setup.rs         │
├─────────────────────────────────────────┤
│  Layer 2  能力层                         │
│  message · client · tools · env          │
│  memory · skill · mode · supervisor     │
├─────────────────────────────────────────┤
│  Layer 1  基础层                         │
│  error.rs · config.rs                   │
└─────────────────────────────────────────┘
```

---

## 模块说明

### Layer 1: 基础层

#### error.rs

全局错误类型：

```rust
pub enum RelayError {
    Config(String),
    Api { status: u16, message: String, body: Option<String> },
    RateLimit(String),
    Auth(String),
    Network(String),
    Tool(String),
    Memory(String),
    Skill(String),
}
```

#### config.rs

配置结构，三层加载（`~/.relay/settings.json` → 环境变量 → `~/.deepseek/config.toml` 回退），原子保存。

| 分组 | 字段 | 默认值 |
|------|------|--------|
| 连接 | `api_key`, `base_url`, `model` | `""` / `"https://api.deepseek.com"` / `"deepseek-chat"` |
| 上下文 | `max_tokens`, `max_context_tokens`, `context_safety_margin`, `max_tool_turns` | `16384` / `128000` / `4000` / `20` |
| 重试 | `retry_max_attempts`, `retry_base_delay`, `retry_max_delay`, `request_timeout` | `3` / `2.0` / `60.0` / `180.0` |
| 工具安全 | `default_shell_timeout`, `max_tool_output`, `max_stderr_output` | `15.0` / `50000` / `10000` |
| 运行模式 | `default_mode` | `"auto"` |
| 防钻 | `anti_stuck_enabled`, `reflect_after_failures`, `max_failures_before_hard_stop`, `compress_tool_history` | `true` / `2` / `4` / `true` |
| 记忆 | `memory_enabled`, `memory_root` | `true` / `"auto"` |
| 技能 | `skill_enabled`, `skill_dirs` | `true` / `["~/.relay/skills"]` |
| 监工(预留) | `dual_agent`, `supervisor_model`, `supervisor_max_turns` | `false` / `""` / `3` |
| 其他 | `enter_sends`, `log_level` | `true` / `"WARNING"` |

---

### Layer 2: 能力层

#### message.rs — 会话管理

类型化消息模型，替代 Python 版的 raw dict：

```rust
pub enum Message {
    System { content: String },
    User { content: String },
    Assistant { content: Option<String>, reasoning_content: Option<String>,
                tool_calls: Option<Vec<ToolCall>> },
    Tool { tool_call_id: String, content: String },
}
```

Session 提供消息增删、token 估算（字符数/4.5 + 固定开销）、上下文修剪、工具历史压缩。

#### client.rs — API 通信

异步 HTTP 流式请求 + SSE 解析 + 指数退避重试。

```rust
pub enum StreamEvent {
    Content(String), Reasoning(String),
    ToolCall { id: String, name: String, args: String },
    Usage { prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 },
    Warning(String), Error(String),
}
```

#### tools.rs — 工具执行

6 个工具，OpenAI function-calling schema：

| # | 工具 | 功能 | 安全限制 |
|---|------|------|---------|
| 1 | `shell` | 执行 shell 命令 | timeout 15s/上限 300s，截断 50KB |
| 2 | `read` | 读取文本文件 | 拒绝二进制，上限 100KB |
| 3 | `write` | 写入文件 | 自动创建父目录 |
| 4 | `glob` | 文件名模式匹配 | 上限 200 条 |
| 5 | `grep` | 正则搜索文件内容 | 上限 200 条，跳过二进制 |
| 6 | `use_skill` | 加载技能完整指令 | 名称必须在 SkillRegistry 中存在 |

#### env.rs — 环境感知

OS / Shell / 工具可用性检测。构建系统提示（含环境信息、模式行为约束、防钻指令、记忆摘要、可用技能列表）。

#### memory.rs — 记忆系统

加载 workspace 下 `memory/` 目录的记忆文件（frontmatter + body），注入系统提示。格式兼容现有 Claude Code memory 体系。

#### skill.rs — 技能系统

扫描 `~/.relay/skills/` 下的 `SKILL.md` 文件。启动时只注入 skill 列表（省 token），模型通过 `use_skill` 工具按需加载完整内容。

#### mode.rs — 运行模式

```rust
pub enum AgentMode {
    Auto,     // 自主执行全部工具
    Confirm,  // 每次 write/shell 工具调用需用户确认
    Plan,     // 先规划再执行：仅允许 read/glob/grep/use_skill
}
```

三种模式通过系统提示 + 工具列表过滤双重约束。`/auto` `/confirm` `/plan` 命令切换。

#### supervisor.rs — 监督者接口（预留）

定义 `Supervisor` trait：
- `review_before_turn()` — 每 turn 开始前检查
- `on_tool_failure()` — 工具失败时检查

首版提供 `NoopSupervisor`（永远不介入），后续可替换为真实 LLM 监工。

---

### Layer 3: 编排层

#### reflect.rs — 防钻牛角尖

| 触发条件 | 动作 |
|---------|------|
| 同一工具连续失败 ≥ 2 次 | 注入反思 system 消息 |
| 总失败 ≥ 4 次 | 硬介入：「回到原始目标」 |
| 连续 3 次同工具+同参数 | 阻断调用 |

不发起额外 LLM 调用——直接向 session 注入 system 消息，模型下一步推理时看到。

#### app.rs — 对话编排

将一个用户输入 → 多轮工具调用 → 最终回复串起来。

```rust
pub enum TurnEvent {
    Content(String),
    Thinking(String),
    ToolCallProposed { name, args, display },  // Confirm 模式：待确认
    ToolCallExecuting { name, args },
    ToolCallResult { display, success },
    Stats { elapsed, tokens, ctx_pct },
    Warning(String), Error(String),
    NeedClarification { question },
    PlanReady { plan },
    ModeChanged { from, to },
    Done,
}
```

流式响应 → SSE 解析 → 工具调用执行（含模式分支判断）→ reflect 检查 → 上下文压缩。

#### setup.rs — 首次设置向导

交互式配置：检测已有配置文件 → 提供导入 → 手动填写（含 default_mode 选择）→ 保存。

---

### Layer 4: 表现层

#### ui.rs — UI 辅助

纯函数：彩色消息行生成、统计行格式化、可视宽度计算、输出截断。

#### term.rs — 终端模式

经典 REPL 回退。crossterm raw mode + 继电器 spinner 动画。ANSI 转义码实现基本的颜色/前缀区分。

#### tui.rs — 全屏 TUI

ratatui 全屏应用。三块布局（chat + input frame + toolbar）。

#### main.rs — 入口

clap 解析参数，组装所有模块，启动 tokio runtime。

---

## 运行模式

### Auto 模式
Agent 自主执行全部工具，无需用户干预。适合简单任务。

### Confirm 模式
每次 write/shell 工具调用暂停，UI 弹出确认框。用户按键：`y` 批准 / `n` 拒绝 / `e` 编辑参数后批准。

### Plan 模式
Agent 仅被授予只读工具（read/glob/grep/use_skill），先调研再产出计划。用户审批后切换到 Auto/Confirm 执行。

**模式切换通过命令或快捷键**（`Ctrl+P` = Plan, `Ctrl+A` = Auto, `Ctrl+Y` = Confirm）。

---

## UI 视觉设计

### 消息类型视觉区分

| 内容类型 | 前缀 | 颜色 | 块样式 |
|---------|------|------|--------|
| Agent 回复 | `│` 蓝色竖线 | 白色正文 | 默认 |
| 推理/思考 | `···` | 灰色斜体 | 默认 |
| 工具调用 | `◆` 橙色 | 橙色粗体 | 默认 |
| 工具结果(成功) | 无 | 暗色 | 灰底等宽块 |
| 工具结果(失败) | `✗` 红色 | 红色 | 红底暗块 |
| Agent 问询 | `?` 黄色 | 黄色高亮 | 虚线边框 |
| 用户消息 | `│` 绿色竖线 | 白色正文 | 右对齐 + `[user]` 标签 |
| 统计行 | 无 | 暗灰 | 右对齐 |
| 计划展示 | `◆` 前缀 | 青色 | 双线边框块 |
| 确认提示 | `!` 前缀 | 黄色 | 反色块 |

### 整体布局

```
┌──────────────────────────────────────────────────────┐
│                                                      │
│  ██  Hello! I found 3 Rust files in the project:     │ ← Agent 消息
│                                                      │
│  ◆ shell: ls *.rs ──────────────────────             │ ← 工具调用
│  ┌──────────────────────────────────────────────┐    │
│  │ main.rs   app.rs   error.rs                  │    │ ← 工具结果
│  └──────────────────────────────────────────────┘    │
│                                                      │
│  ██  Would you like me to explain the structure?     │ ← Agent 回复
│                                                      │
│  ?  Which directory should I search in?              │ ← Agent 问询
│                                                      │
│                               src/ │ [zt]            │ ← 用户消息
│                                                      │
│  ┌─ Input ──────────────────────────────────────┐   │
│  │ │ >                                           │   │
│  └──────────────────────────────────────────────┘   │
│  Relay | deepseek-chat | ● Auto | ctx:87% | ...     │ ← Toolbar
└──────────────────────────────────────────────────────┘
```

### Toolbar

```
Relay | deepseek-chat | ● Auto | ctx:87% | 3.2s 142tok
```

- 模式指示：`● Auto`（绿）/ `○ Confirm`（黄）/ `◇ Plan`（青）
- 处理中显示 `⏳ processing`（闪烁）
- 上下文 < 20% 时 `ctx` 变红

---

## 技术选型

| 层面 | 选型 |
|------|------|
| TUI | ratatui + crossterm |
| HTTP | reqwest (rustls-tls) |
| 异步 | tokio |
| 序列化 | serde + serde_json |
| CLI | clap |
| 错误处理 | thiserror + anyhow |
| 文件搜索 | regex + walkdir |
| 配置兼容 | toml（仅读旧 `~/.deepseek/config.toml`） |

---

## 依赖关系

```
                   基础层
              ┌──────┴──────┐
              │ error  config│
              └──────┬──────┘
                     │
       能力层（8 个模块彼此独立）
       ┌───────┬───────┬───────┬───────┐
       │message│client │tools  │env    │
       │memory │skill  │mode   │supervisor│
       └───────┴───────┴───┬───┴───────┘
                           │
                   编排层
              ┌────────┬───┴───┬────────┐
              │reflect │ app   │ setup  │
              └────────┴───┬───┴────────┘
                           │
                   表现层
              ┌──────┬────┴───┬──────┐
              │ ui   │ term   │ tui  │
              └──────┴────┬───┴──────┘
                          │
                       main.rs
```
