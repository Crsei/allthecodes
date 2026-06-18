# PTY E2E 测试方案对比

以测试 `claude-code-rs` 终端为例，对比 5 种语言/工具方案的实际可行性。

## 测试目标

PTY 测试的核心需求：

1. 在真实伪终端（`isatty()=true`）中启动二进制
2. 捕获完整终端输出（含 ANSI 转义序列）
3. 模拟键盘输入（文字、Enter、Ctrl+C）
4. 自动响应 crossterm 的 DSR 查询（`\x1b[6n`），否则进程会阻塞
5. 等待特定文本出现（带超时）
6. 跨平台：Windows ConPTY + Linux PTY

---

## 方案一：Python + pexpect

### 库

- `pexpect`（Linux/macOS 原生 PTY）
- `wexpect` 或 `pexpect.popen_spawn`（Windows）
- `pytest` 测试框架

### 示例

```python
import pexpect

def test_version():
    child = pexpect.spawn('./target/release/claude-code-rs', ['-V'], timeout=10)
    child.expect('claude-code-rs')
    child.wait()
    assert child.exitstatus == 0

def test_tui_starts():
    child = pexpect.spawn(
        './target/release/claude-code-rs',
        ['-C', '/tmp', '--permission-mode', 'bypass'],
        timeout=5,
        dimensions=(40, 120),  # rows, cols
    )
    child.expect(pexpect.TIMEOUT, timeout=3)
    child.sendcontrol('c')
    child.expect(pexpect.EOF, timeout=5)

def test_simple_chat():
    child = pexpect.spawn(
        './target/release/claude-code-rs',
        ['-C', '/tmp', '--permission-mode', 'bypass'],
        timeout=60,
        dimensions=(40, 120),
    )
    child.expect(r'[>$]', timeout=5)
    child.sendline('Say exactly: PTY_TEST_OK')
    child.expect('PTY_TEST_OK', timeout=60)
    child.sendcontrol('c')
    child.expect(pexpect.EOF, timeout=5)
```

### DSR 处理

pexpect **不会**自动响应 `\x1b[6n`。需要手动处理，但会和主线程的 `expect()` 冲突（两个线程同时读 stdout）。需要用 `pexpect.expect_list()` 同时匹配 DSR 和目标文本，实现复杂。

### Windows 支持

| 模块 | Windows 支持 | 说明 |
|------|-------------|------|
| `pexpect.spawn` | **不支持** | 依赖 Unix `pty.fork()` |
| `pexpect.popen_spawn` | 部分 | 不是真 PTY，`isatty()=false` |
| `wexpect` | 实验性 | 封装 ConPTY，最后更新 2022，维护不活跃 |
| WSL | 可用 | 但测试的是 Linux 二进制，不是 Windows |

### 优缺点

```
+ API 最简洁：spawn/expect/sendline 三步搞定
+ 模式匹配强大：支持正则、多模式匹配
+ 成熟稳定（20+ 年历史）
+ pytest 集成自然

- Windows 上不能用真 PTY（pexpect.spawn 不支持 Windows）
- wexpect 维护不活跃，ConPTY 支持不完善
- DSR 自动响应需要额外 workaround，实现复杂
- 需要额外安装 Python 环境
- 不能和 cargo test 统一运行
```

---

## 方案二：Node.js/Bun + node-pty

### 库

- `node-pty`（Microsoft 维护，VS Code 终端的底层库）
- `strip-ansi`（ANSI 序列剥离）
- `@xterm/headless` + `@xterm/addon-serialize`（可选：渲染级验证）
- `bun:test` 或 `vitest` 测试框架

### 示例

```typescript
import * as pty from 'node-pty'
import stripAnsi from 'strip-ansi'
import { test, expect } from 'bun:test'

class PtyHarness {
  private proc: pty.IPty
  private output = ''
  private listeners: Array<{
    pattern: RegExp
    resolve: (match: RegExpMatchArray) => void
    reject: (err: Error) => void
    timer: ReturnType<typeof setTimeout>
  }> = []

  constructor(binary: string, args: string[], options?: Partial<pty.IPtyForkOptions>) {
    this.proc = pty.spawn(binary, args, {
      name: 'xterm-256color',
      cols: 120,
      rows: 40,
      ...options,
    })
    this.proc.onData((data: string) => {
      this.output += data
      this.checkWaiters()
    })
  }

  private checkWaiters(): void {
    for (let i = this.listeners.length - 1; i >= 0; i--) {
      const l = this.listeners[i]
      const match = this.output.match(l.pattern)
      if (match) {
        clearTimeout(l.timer)
        this.listeners.splice(i, 1)
        l.resolve(match)
      }
    }
  }

  waitFor(pattern: RegExp | string, timeoutMs = 10000): Promise<RegExpMatchArray> {
    const re = typeof pattern === 'string' ? new RegExp(pattern) : pattern
    const existing = this.output.match(re)
    if (existing) return Promise.resolve(existing)

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.listeners = this.listeners.filter(l => l.resolve !== resolve)
        reject(new Error(
          `Timeout (${timeoutMs}ms) waiting for: ${re}\n` +
          `Last 500 chars: ${this.output.slice(-500)}`
        ))
      }, timeoutMs)
      this.listeners.push({ pattern: re, resolve, reject, timer })
    })
  }

  type(text: string): void { this.proc.write(text) }
  enter(): void { this.proc.write('\r') }
  interrupt(): void { this.proc.write('\x03') }
  get textOutput(): string { return stripAnsi(this.output) }

  async destroy(): Promise<number> {
    return new Promise(resolve => {
      this.proc.onExit(({ exitCode }) => {
        for (const l of this.listeners) {
          clearTimeout(l.timer)
          l.reject(new Error('Process exited'))
        }
        this.listeners = []
        resolve(exitCode)
      })
      this.proc.kill()
    })
  }
}

// 测试用例
test('version flag', async () => {
  const h = new PtyHarness(BINARY, ['-V'])
  await h.waitFor(/claude-code-rs/)
  expect(h.textOutput).toContain('claude-code-rs')
  await h.destroy()
})
```

### DSR 处理

**关键发现：Windows ConPTY 自动处理 DSR。** ConPTY 拦截 `\x1b[6n` 并自动向子进程回写 `\x1b[{row};{col}R`。这意味着：

- **Windows**：DSR 完全透明，无需手动处理，crossterm TUI 正常启动
- **Linux**：原始 PTY 不会响应 DSR，需要在 `onData` 中手动检测并回写

### xterm-headless 渲染级验证（可选扩展）

如果需要验证"用户实际看到什么"（而非正则匹配文本），可以用 `@xterm/headless`：

```typescript
import { Terminal } from '@xterm/headless'

const term = new Terminal({ cols: 120, rows: 40 })
// 将 PTY 输出喂给虚拟终端
proc.onData((data: string) => term.write(data))

// 获取渲染后的屏幕文本
function getScreenText(): string {
  const lines: string[] = []
  const buf = term.buffer.active
  for (let row = 0; row < term.rows; row++) {
    const line = buf.getLine(row)
    lines.push(line ? line.translateToString(true) : '')
  }
  return lines.join('\n')
}
```

这正确处理光标移动、行换行、交替屏幕缓冲区、擦除操作等——所有正则剥离无法正确处理的场景。

### Windows 支持

| 特性 | 支持 |
|------|------|
| Windows ConPTY | **原生支持**（VS Code 每天在数百万 Windows 机器上使用） |
| `isatty()=true` | 是 |
| DSR 自动响应 | **Windows 自动**（ConPTY 处理），Linux 需手动 |
| Linux PTY | 是 |
| macOS PTY | 是 |

### 与项目的契合度

- `ui/` 已是 Bun workspace，可直接在 `ui/` 中添加 PTY 测试
- `ui/src/ipc/client.ts` 已有 `RustBackend` 类，headless 测试可直接复用
- node-pty 是 native addon（需要 node-gyp + C++ 编译器），Bun 1.1+ 支持但可能有 edge case

### 优缺点

```
+ Windows ConPTY 原生支持（VS Code 验证的稳定性）
+ DSR 在 Windows 上完全自动
+ 事件驱动的 async/await 模型，天然适合等待模式
+ 项目已有 Bun 环境，TypeScript 类型安全
+ strip-ansi / xterm-headless 生态成熟
+ xterm-headless 可做渲染级验证（像素级准确）

- node-pty 是 native addon，需要 C++ 编译器（node-gyp）
- Bun 对 node-pty 的兼容性有 edge case（可回退到 Node.js + vitest）
- 不能和 cargo test 统一运行
- Linux 上仍需手动 DSR 响应
```

---

## 方案三：Go + creack/pty / go-pty

### 库

- `github.com/creack/pty`（Unix 专用，最成熟）
- `github.com/aymanbagabas/go-pty`（跨平台，Unix + Windows ConPTY）
- `go test` 测试框架

### 示例（使用 go-pty 跨平台）

```go
package e2e_test

import (
    "bytes"
    "io"
    "strings"
    "testing"
    "time"

    gopty "github.com/aymanbagabas/go-pty"
)

func TestCLIVersion(t *testing.T) {
    ptmx, err := gopty.New()
    if err != nil { t.Fatal(err) }
    defer ptmx.Close()

    cmd := exec.Command("./target/release/claude-code-rs", "-V")
    cmd.Stdin = ptmx.Slave()
    cmd.Stdout = ptmx.Slave()
    cmd.Stderr = ptmx.Slave()
    if err := cmd.Start(); err != nil { t.Fatal(err) }
    ptmx.Slave().Close()

    output, err := readUntil(ptmx, "claude-code-rs", 5*time.Second)
    if err != nil { t.Fatal(err) }
    if !strings.Contains(stripANSI(output), "claude-code-rs") {
        t.Errorf("unexpected output: %s", output)
    }
}

// DSR 自动响应 + 超时读取
func readUntil(ptmx io.ReadWriter, needle string, timeout time.Duration) (string, error) {
    var buf bytes.Buffer
    tmp := make([]byte, 4096)
    deadline := time.After(timeout)
    for {
        select {
        case <-deadline:
            return buf.String(), fmt.Errorf("timeout waiting for %q", needle)
        default:
        }
        n, err := ptmx.Read(tmp)
        if n > 0 {
            chunk := tmp[:n]
            // DSR 自动响应
            for {
                idx := bytes.Index(chunk, []byte("\x1b[6n"))
                if idx == -1 { buf.Write(chunk); break }
                buf.Write(chunk[:idx])
                ptmx.Write([]byte("\x1b[1;1R"))
                chunk = chunk[idx+4:]
            }
            if strings.Contains(stripANSI(buf.String()), needle) {
                return buf.String(), nil
            }
        }
        if err != nil { return buf.String(), err }
    }
}
```

### Windows 支持

| 库 | Windows | 说明 |
|----|---------|------|
| `creack/pty` | **不支持** | 仅 Unix（依赖 `posix_openpt`） |
| `go-pty` | **支持** | 抽象层：Unix 用 creack/pty，Windows 用 ConPTY |
| `conpty` | 部分 | 轻维护的 ConPTY 绑定 |

**关键发现**：`go-pty` 是唯一可行的跨平台选项，但相对年轻，API 可能变动。Windows ConPTY 路径的成熟度不如 node-pty。

### DSR 处理

需要在 goroutine 的读取循环中手动检测 `\x1b[6n` 并回写。**注意**：DSR 序列可能跨 read 边界分割（`\x1b[6` 和 `n` 分两次到达），需要状态机或尾部缓冲处理。

### 优缺点

```
+ goroutine 天然适合并发读写 PTY
+ go test 零配置，单二进制测试
+ 交叉编译简单
+ ANSI 剥离有 acarl005/stripansi

- creack/pty 不支持 Windows
- go-pty 跨平台但年轻，Windows 路径不够成熟
- 没有内置 expect 库（Netflix/go-expect 已归档且仅 Unix）
- 需要额外安装 Go 工具链
- 不能和 cargo test 统一运行
- ReadDeadline 在 ConPTY 上不可用，必须用 goroutine + channel
```

---

## 方案四：Shell (expect/unbuffer)

### 工具

- `expect`（Tcl，Unix 经典 PTY 自动化）
- `unbuffer`（expect 附带，强制 PTY 分配）
- `script`（Unix 终端录制）

### 示例

```tcl
#!/usr/bin/expect -f
set timeout 30
log_file -noappend test_output.log

spawn ./target/release/claude-code-rs -C /tmp --permission-mode bypass
sleep 3

# 自动响应 DSR（expect 原生支持后台匹配）
expect_background {
    -re "\033\\\[6n" { send "\033\[1;1R" }
}

send "Say exactly: PTY_TEST_OK\r"
expect {
    "PTY_TEST_OK" { puts "PASS" }
    timeout { puts "FAIL: timeout"; exit 1 }
}

send \x03
expect eof
```

### Windows 支持

| 工具 | Windows |
|------|---------|
| `expect` 原生 | **不支持** |
| MSYS2 `expect` | 用 MSYS PTY，非 ConPTY。原生 MSVC 二进制 `isatty()=false` |
| Git Bash | 同 MSYS |
| WSL | 可用但测试 Linux 二进制 |
| `winpty` | 不同于 expect，不可编程 |

**关键发现**：MSYS2 的 `expect` 创建的是 MSYS/Cygwin 伪终端，原生 Windows 二进制通过它看到的是管道而非控制台，`isatty()` 返回 `false`，TUI 不会初始化。

### DSR 处理

`expect_background` 原生支持后台模式匹配，这是所有方案中**最优雅**的 DSR 处理方式——但仅限 Unix。

### 优缺点

```
+ 最简洁语法：spawn/expect/send 三行
+ expect_background 原生处理 DSR（最优雅）
+ 零编译，脚本即可运行
+ Unix 上最成熟

- Windows 基本不可用（MSYS PTY 对原生二进制无效）
- Tcl 语言小众，复杂逻辑难写
- 超时精度仅 1 秒
- 无结构化测试报告
- 最脆弱：依赖精确输出时序
- ANSI 剥离需要手写 Tcl 正则（不完整）
```

---

## 方案五：Rust + portable-pty（当前方案）

### 当前实现分析

现有 `tests/e2e_pty.rs` 共 462 行：

| 部分 | 行数 | 说明 |
|------|------|------|
| 日志基础设施 | ~105 | LogDirs、聚合日志、时间戳目录 |
| PtySession 结构体 | ~190 | spawn、send_line、send_ctrl_c、finish、wait_for_text |
| DSR 自动响应 | ~25 | reader 线程中检测 `\x1b[6n` |
| 辅助函数 | ~20 | find_subsequence、CapturedOutput |
| 测试用例 | ~120 | 6 个测试 |
| **基础设施占比** | **~74%** | 测试代码仅占 26% |

### 实际踩过的坑（4 个 Windows ConPTY 问题）

1. **`take_writer()` 只能调用一次** — `MasterPty::take_writer()` 消费 handle，第二次 panic。必须 `Arc<Mutex<Box<dyn Write + Send>>>` 共享。

2. **子进程快速退出丢输出** — `--version` 瞬间退出，如果立刻 `drop(slave)`，ConPTY 管道在 reader 读到数据前就关闭。实测：drop slave 后只捕获 4 bytes（一个 DSR 查询），保留 slave 后捕获 97 bytes。必须保留 slave 到 `finish()`。

3. **Unicode 边界 panic** — `text[..500]` 可能切到多字节字符中间（如 `█` 占 3 bytes）。需 `char_indices()` 安全截断。

4. **crossterm DSR 阻塞** — 必须在 reader 线程中扫描尾部缓冲检测 `\x1b[6n` 并回复。实现需要处理序列跨 read 边界的情况。

### 新增依赖

| Crate | 版本 | 位置 | 用途 |
|-------|------|------|------|
| `portable-pty` | 0.9 | dev-dependencies | ConPTY/PTY 抽象 |
| `strip-ansi-escapes` | 0.2 | dependencies（已有） | ANSI 剥离 |
| `chrono` | 0.4 | dependencies（已有） | 日志时间戳 |

### 优缺点

```
+ cargo test 统一运行，和其他 e2e 测试一起
+ 直接用 assert_cmd::cargo_bin() 找二进制
+ 类型安全，编译时发现错误
+ portable-pty 支持 Windows ConPTY + Linux
+ 不需要额外语言/运行时
+ 与项目同语言，贡献者易维护

- 基础设施代码量大（340 行 infra / 120 行测试）
- Windows ConPTY 有 4 个 edge case 需要 workaround
- portable-pty API 不如 pexpect/node-pty 友好
- 没有内置 expect/pattern-matching 原语
- DSR 跨 read 边界处理需要尾部缓冲
```

---

## 综合对比

| 维度 | Python pexpect | Node node-pty | Go go-pty | Shell expect | Rust portable-pty |
|------|---------------|---------------|-----------|-------------|-------------------|
| **Windows ConPTY** | ❌ | ✅ 原生 | ⚠️ 年轻 | ❌ | ✅ |
| **Linux PTY** | ✅ | ✅ | ✅ | ✅ | ✅ |
| **DSR 自动响应** | 手动，复杂 | Win 自动 / Linux 手动 | 手动 goroutine | expect_background | 手动 reader 线程 |
| **API 简洁度** | ★★★★★ | ★★★★ | ★★★ | ★★★★★ | ★★ |
| **cargo test 集成** | ❌ | ❌ | ❌ | ❌ | ✅ |
| **项目已有环境** | ❌ 需装 Python | ✅ Bun 已有 | ❌ 需装 Go | ❌ | ✅ Rust 已有 |
| **基础设施代码量** | ~30 行 | ~60 行 | ~80 行 | ~15 行 | ~340 行 |
| **渲染级验证** | ❌ | ✅ xterm-headless | ❌ | ❌ | ❌ |
| **社区/维护** | 成熟 | 微软维护 | go-pty 年轻 | 成熟但小众 | portable-pty 活跃 |
| **CI 安装成本** | pip install | bun install + node-gyp | go mod | apt install | 零 |

---

## 推荐策略

### 核心发现

1. **支持 Windows ConPTY 的成熟方案只有两个**：Rust portable-pty 和 Node node-pty
2. **node-pty 在 Windows 上 DSR 自动响应**（ConPTY 处理），Rust 需要手动
3. **node-pty + xterm-headless 可以做渲染级验证**，这是其他方案都做不到的
4. **`--headless` JSONL 模式覆盖 80% 的测试需求**，不需要 PTY

### 本项目推荐的三层测试架构

```
第一层：cargo test --test e2e_terminal     (42 个测试)
  → JSONL IPC 协议正确性，不需要 PTY
  → 覆盖：引擎、工具、权限、命令、token 追踪
  → 优势：稳定、快速、跨平台、无 ANSI 干扰

第二层：cargo test --test e2e_pty          (6 个测试)
  → Rust PTY smoke test
  → 覆盖：二进制启动、TUI 基本渲染、快速路径
  → 优势：cargo test 统一、不需要额外工具链

第三层：cd ui && bun test                  (可选扩展)
  → node-pty + xterm-headless TUI 渲染验证
  → 覆盖：布局、颜色、交替屏幕、resize 行为
  → 优势：渲染级准确、async 友好、DSR 自动
  → 时机：当 TUI 渲染 bug 成为痛点时再加
```

### 不推荐的方案

- **Python pexpect**：Windows 无法使用真 PTY，本项目主开发环境是 Windows
- **Go go-pty**：需要额外工具链，go-pty 在 Windows 上不够成熟
- **Shell expect**：MSYS PTY 对原生二进制无效，Tcl 小众，超时精度差
