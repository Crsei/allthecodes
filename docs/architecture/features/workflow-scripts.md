---
title: "工作流脚本"
description: "npm 发布流水线脚本和自动化工作流，从构建到发布的完整工具链。"
keywords: ["workflow", "脚本", "npm", "发布", "构建", "CI"]
---

## 概述

工作流脚本系统为 allthecodes 提供自动化发布流水线能力。当前主要包含 npm 包的构建和发布脚本，支持跨平台二进制分发的完整工作流。

## 脚本清单

### npm 发布流水线

#### `scripts/build_npm_package.py`

npm 包构建脚本，负责将 Rust 二进制编译产物打包为 npm 包：

- **平台包定义**：为每个目标平台创建独立 npm 包（如 `allthecodes-linux-x64`、`allthecodes-linux-arm64`、`allthecodes-darwin-x64`、`allthecodes-darwin-arm64`、`allthecodes-windows-x64`）
- **包结构**：
  - 平台特定包（`allthecodes-{platform}-{arch}`）：包含编译后的二进制文件和 `package.json`
  - 主包（`allthecodes`）：作为入口点，自动检测平台并代理到对应平台包
- **npm tag**：每个平台包使用 `npm_tag` 标记（如 `linux-x64`）
- **target triple**：指定 Rust 交叉编译目标（如 `x86_64-unknown-linux-gnu`）

```python
ALLTHECODES_PLATFORM_PACKAGES = {
    "allthecodes-linux-x64": {
        "npm_name": "allthecodes-linux-x64",
        "npm_tag": "linux-x64",
        "target_triple": "x86_64-unknown-linux-gnu",
        "os": "linux",
        "cpu": "x64",
    },
    # ... 其他平台
}
```

#### `scripts/stage_npm_packages.py`

npm 包暂存脚本，将构建好的包组织到发布目录：

- 加载 `build_npm_package.py` 中的包定义
- 自动检测主机平台
- 创建暂存目录结构
- 复制二进制文件和元数据
- 验证包完整性

## 架构

### 包发布流程

```
代码变更 → Rust 编译 → 跨平台二进制
      │
      ▼
build_npm_package.py
      │
      ├── 为每个平台创建 npm 包结构
      ├── 生成 package.json（含 os/cpu 过滤）
      ├── 复制二进制文件到包目录
      │
      ▼
stage_npm_packages.py
      │
      ├── 组织暂存目录
      ├── 验证产物完整性
      │
      ▼
npm publish（各平台包 + 主包）
      │
      ▼
用户通过 npm install allthecodes 安装
      │
      ▼
主包的 install.js 检测平台 → 下载对应平台包
```

### 平台检测

主包通过 `install.js` 或生命周期脚本进行平台检测：
- 检测 `os` 和 `cpu`
- 映射到对应的平台特定包名
- 安装正确平台的二进制文件

## 设计决策

1. **平台特定包 + 主包代理**：每个操作系统/架构组合有独立 npm 包，主包作为代理自动选择正确的平台包。避免用户手动指定平台
2. **Python 脚本**：使用 Python 而非 shell 脚本实现跨平台兼容性，避免 POSIX 特有命令在 Windows 上的兼容问题
3. **npm tag 策略**：每个平台包使用独立 tag，便于按平台筛选和安装

## 使用方式

```bash
# 构建所有平台 npm 包
python scripts/build_npm_package.py --all

# 构建特定平台
python scripts/build_npm_package.py --platform linux-x64

# 暂存包到发布目录
python scripts/stage_npm_packages.py --output ./dist

# 发布（需要 npm 认证）
cd dist/allthecodes && npm publish
cd dist/allthecodes-linux-x64 && npm publish
# ... 发布其他平台包
```

## 相关文件

| 文件 | 职责 |
|------|------|
| `scripts/build_npm_package.py` | npm 包构建脚本 |
| `scripts/stage_npm_packages.py` | npm 包暂存和发布准备 |
| `scripts/__pycache__/` | Python 缓存目录 |
| `crates/allthecodes-plugins/src/lib.rs` | 插件注册表（插件工作流的另一种形式） |
| `crates/allthecodes-plugins/src/installation.rs` | 插件安装逻辑 |
| `crates/allthecodes-plugins/src/marketplace.rs` | 插件市场 |
| `crates/allthecodes-plugins/src/versioning.rs` | 插件版本管理 |
