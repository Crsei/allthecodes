# PTY E2E 测试

## 概述

`tests/e2e_pty.rs` 使用 `portable-pty` crate 在 Windows ConPTY 伪终端中启动 `claude-code-rs`，完整捕获终端渲染输出并保存到 `logs/` 目录。

与 `e2e_terminal.rs`（`--headless` JSONL 协议测试）互补：headless 测试验证结构化消息，PTY 测试验证真实终端下的渲染行为。

## 架构

```
┌─────────────────────────────────────────────────────────┐
│  cargo test (e2e_pty)                                   │
│                                                         │
│  ┌──────────────┐     ConPTY (Windows 伪终端)           │
│  │  PtySession   │                                      │
│  │               │    ┌─────────┐    ┌────────────────┐ │
│  │  writer ──────────►│  slave  │───►│ claude-code-rs │ │
│  │  (Arc<Mutex>) │    │  (输入) │    │   子进程        │ │
│  │               │    └─────────┘    │                │ │
│  │               │    ┌─────────┐    │  stdout/stderr │ │
│  │  reader ◄──────────│ master  │◄───│  (终端渲染)    │ │
│  │  (后台线程)   │    │  (输出) │    └────────────────┘ │
│  │     │         │    └─────────┘                       │
│  │     ▼         │                                      │
│  │  buffer ──────────► logs/*.raw  (含 ANSI)            │
│  │  (Arc<Mutex>) │                                      │
│  │     │         │                                      │
│  │     ▼         │                                      │
│  │  strip_ansi ──────► logs/*.log  (纯文本)             │
│  └──────────────┘                                       │
└─────────────────────────────────────────────────────────┘
```

## 核心原理

### 1. 创建伪终端

`portable-pty` 调用 Windows ConPTY API 创建一对管道：

- **slave** — 子进程看到的"终端"，`isatty()` 返回 `true`
- **master** — 测试代码持有的控制端，读 master = 子进程的输出，写 master = 子进程的输入

子进程完全认为自己在真实终端中运行，会正常初始化 TUI、渲染 box-drawing 字符、输出 ANSI 颜色序列等。

### 2. 后台 reader 线程

spawn 后立即启动一个线程，持续从 master 读取数据写入共享 `buffer`：

```rust
let reader_thread = std::thread::spawn(move || {
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,        // EOF
            Ok(n) => {
                buf_clone.lock().unwrap().extend_from_slice(&chunk[..n]);
                // + DSR 自动响应逻辑 (见下文)
            }
            Err(_) => break,
        }
    }
});
```

所有终端输出（包括 ANSI 转义序列）被原样捕获到内存中的 `Arc<Mutex<Vec<u8>>>`。

### 3. 自动响应 DSR 查询（关键）

crossterm 启动时发送 `\x1b[6n`（Device Status Report）向终端询问光标位置，然后**阻塞等待回复**。真实终端会回复 `\x1b[{row};{col}R`。

如果不响应，子进程会永远阻塞。reader 线程检测到该序列后，通过 shared writer 写回假回复：

```
子进程: "光标在哪？" → \x1b[6n → master 输出端
reader 线程: 检测到 → writer → \x1b[1;1R → slave 输入端 → 子进程 stdin
子进程: "收到，光标在 (1,1)" → 继续运行
```

实现方式：writer 被包装为 `Arc<Mutex<Box<dyn Write + Send>>>`，reader 线程和主线程共享同一个 writer。

### 4. 输入模拟

通过 writer 写入 master，ConPTY 把数据送到子进程的 stdin，与用户键盘输入等价：

```rust
// 发送一行文字 + 回车
session.send_line("Say exactly: hello");

// 发送 Ctrl+C (ETX 0x03)
session.send_ctrl_c();
```

### 5. finish() 收尾流程

```
等待子进程退出 (poll try_wait, 有 timeout)
        ↓
sleep 200ms (让 ConPTY flush 缓冲区)
        ↓
drop slave (触发 reader 线程 EOF)
        ↓
drop writer
        ↓
join reader 线程
        ↓
buffer → logs/{test_name}.raw  (原始字节，含 ANSI 转义)
buffer → strip_ansi → logs/{test_name}.log  (纯文本)
```

**关键：必须保留 slave handle 直到 finish()**。如果 spawn 后立刻 drop slave，Windows ConPTY 在子进程快速退出时会丢失缓冲区数据。这是在实际调试中发现的问题——drop slave 后 `--version` 只捕获 4 bytes（一个 DSR 查询），保留 slave 后正确捕获 97 bytes。

## 与 headless 测试对比

| 维度 | `--headless` (e2e_terminal.rs) | PTY (e2e_pty.rs) |
|---|---|---|
| 子进程模式 | piped stdio, JSONL 协议 | 真实终端 (ConPTY) |
| 捕获内容 | 结构化 JSON 消息 | 完整终端渲染画面 |
| 能测 TUI 布局 | 不能 | 能 |
| 能测 ANSI 颜色/样式 | 不能 | 能 |
| 能发现渲染 bug | 不能 | 能 |
| 断言方式 | JSON 字段匹配 | 文本/正则匹配 |
| 适用场景 | IPC 协议正确性 | 终端用户体验 |

## 测试列表

### Offline（无需 API key）

| 测试 | 说明 | 典型输出大小 |
|---|---|---|
| `pty_version_flag` | `-V` 版本输出 | ~21 bytes plain |
| `pty_init_only` | `--init-only` 初始化退出 | ~296 bytes |
| `pty_dump_system_prompt` | 系统提示词完整捕获 | ~22 KB |
| `pty_tui_starts` | TUI 启动渲染、Ctrl+C 退出 | ~4-7 KB raw |

### Live（需要 API key，默认 `#[ignore]`）

| 测试 | 说明 |
|---|---|
| `live_pty_simple_chat` | TUI 中发送 prompt，等待响应 |
| `live_pty_print_mode` | `-p` print 模式完整捕获 |

## 运行方式

```bash
# 运行所有 offline 测试
cargo test --test e2e_pty

# 运行单个测试（附输出）
cargo test --test e2e_pty pty_tui_starts -- --nocapture

# 运行 live 测试（需要 API key）
cargo test --test e2e_pty -- --ignored --nocapture
```

## 日志输出

每次运行后在 `logs/` 目录生成：

- `{test_name}.raw` — 原始字节，含 ANSI 转义序列，可用 `xxd` 或支持 ANSI 的查看器打开
- `{test_name}.log` — strip ANSI 后的纯文本，可直接阅读

`.gitignore` 已配置排除这些文件。

## 依赖

```toml
[dev-dependencies]
portable-pty = "0.9"     # ConPTY 伪终端
# strip-ansi-escapes 已在 dependencies 中
```

## 踩过的坑

### 1. `take_writer()` 只能调用一次

`MasterPty::take_writer()` 消费 writer handle，第二次调用会 panic。解法：spawn 时取出 writer，包装为 `Arc<Mutex<Box<dyn Write + Send>>>`，reader 线程和主线程共享。

### 2. 子进程快速退出时丢失输出

`--version` 这样的快路径命令瞬间退出，如果 spawn 后立刻 `drop(slave)`，ConPTY 管道在 reader 线程读到数据之前就关闭了。解法：保留 slave handle，在 `finish()` 中等子进程退出 + sleep 200ms 后再 drop。

### 3. Unicode 边界 panic

`output.text()[..500]` 如果 500 恰好落在多字节字符中间（如 `█` 占 3 bytes），会 panic。解法：用 `char_indices()` 找安全的截断位置。

### 4. crossterm DSR 阻塞

crossterm 检测到真实终端后发送 `\x1b[6n` 查询光标位置并阻塞。普通 piped 测试不会触发（`isatty()=false`），但 PTY 中 `isatty()=true`，必须自动响应。
