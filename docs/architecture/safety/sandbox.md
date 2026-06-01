---
title: "沙箱机制 - 权限系统之外的第二道防线"
description: "详解 allthecodes 的沙箱设计：SandboxMode 三种隔离级别、SandboxPolicy 的组装策略、Linux/macOS/Windows 平台差异、filesystem path resolver、network policy 域名白名单、preflight shell command 预检。"
keywords: ["沙箱", "sandbox", "bubblewrap", "seatbelt", "filesystem", "network policy", "SandboxMode"]
---

## 一句话结论

沙箱不是用来替代权限系统，而是给 shell 命令再套一层 OS 级能力边界。权限系统决定"要不要执行"，沙箱决定"执行后最多能碰什么"。

## 三种沙箱模式

`mode.rs` 定义 `SandboxMode` 枚举：

| 模式 | 值 | 写权限 | 说明 |
|------|-----|--------|------|
| **ReadOnly** | `"read-only"` | 禁止 | 完全只读，连工作目录也不允许写 |
| **Workspace** | `"workspace"` | 工作区内 | 默认模式，工作区和白名单路径可写 |
| **Full** | `"full"` | 允许 | 沙箱禁用，命令无约束运行 |

当 `enabled=false` 时强制为 `Full` 模式。

## 策略组装

`policy.rs` 的 `SandboxPolicyBuilder` 从多个来源合并策略：

1. `SandboxSettings` — 用户配置文件中的沙箱设置
2. `--no-network` CLI 参数 — 强制禁用网络
3. `permissions.additionalDirectories` — 额外工作目录
4. 权限 allow/deny 规则 — `Read(...)`、`Edit(...)` 等规则翻译为文件系统路径

最终生成 `SandboxPolicy`，包含 `enabled`、`mode`、`fail_if_unavailable`、`paths`（PathResolver）、`network`（NetworkPolicy）、`availability` 等字段。

## 文件系统策略

`filesystem.rs` 的 `PathResolver` 解析路径规则：

- 支持三前缀：`/absolute/path`（绝对路径）、`~/relative`（home 目录）、`./relative`（工作目录相对）
- 检查优先级：`denyWrite` → `allowWrite` → 工作目录/额外目录 → 拒绝
- 读检查：`allowRead` 覆盖 `denyRead`，默认允许读

## 网络策略

`network.rs` 的 `NetworkPolicy` 提供三层检查：

1. **`disabled`** — 全局禁用网络，匹配 `--no-network`
2. **`allowed_domains`** — 域名白名单，支持精确匹配、`*.example.com` 通配符、子域名自动匹配
3. **Shell 命令预检** — `check_shell_command` 分析常见网络命令（curl、wget、git clone、ssh、npm install、cargo install 等），从参数中提取目标域名

## 平台差异

`availability.rs` 检测 OS 级沙箱可用性：

| 平台 | 机制 | 检测方式 |
|------|------|---------|
| Linux / WSL2 | bubblewrap（`bwrap`） | `which::which("bwrap")` |
| macOS | sandbox-exec（Seatbelt） | `/usr/bin/sandbox-exec` 存在性 |
| Windows | 不支持 OS 级原语 | 始终 Unavailable |

`fail_if_unavailable` 控制缺失时是 warning 还是硬错误。

## 运行器实现

`runner.rs` 的三个运行器：

### BubblewrapRunner（Linux）
- 使用 `--unshare-user-try --unshare-ipc --unshare-pid` 等命名空间隔离
- `--ro-bind / /` 提供只读根文件系统
- 工作目录根据模式使用 `--bind`（RW）或 `--ro-bind`（RO）
- 额外 allowWrite 路径使用 `--bind-try`
- `--unshare-net` 在禁用网络时阻断网络
- denyWrite 路径使用 `--ro-bind-try` 覆盖为只读
- `--chdir` 设置工作目录

### SeatbeltRunner（macOS）
- 构建 Seatbelt profile（`(version 1)(deny default)`）
- `(allow file-read*)` 默认允许读
- 根据模式添加 `(allow file-write* (subpath ".."))`
- 网络使用 `(deny network*)` 或 `(allow network*)`

### UnsupportedRunner（Windows/其他）
- 默认 pass-through，仅 Rust 级策略检查
- `fail_if_unavailable=true` 时返回 `PrimitiveUnavailable` 错误

## 命令预检

`preflight_shell_command` 在 spawn 之前进行最佳努力策略检查：

- 网络策略检查（对 `--no-network` 和域名白名单生效，即使 OS 原语不可用）
- 提取 shell 写入目标（重定向目标、`touch`/`mkdir`/`cp` 等命令的参数）
- ReadOnly 模式下拒绝写入
- 检查 denyWrite 列表和 workspace bound

## 允许/排除命令

`SandboxPolicy` 支持两类命令列表：

- **`excluded_commands`** — 这些命令跳过沙箱直接执行
- **`allowed_commands`** — 这些命令在 workspace 模式下自动批准

两者的匹配方式不同：excluded 使用宽松的前缀匹配，allowed 使用严格的 argv 前缀匹配且拒绝非 argv 形状的 shell 语法。
