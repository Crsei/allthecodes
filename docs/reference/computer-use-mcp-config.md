# Computer Use MCP 配置指南

## 概述

cc-rust 通过外置 MCP (Model Context Protocol) 服务器实现 Computer Use 能力。
模型可以调用截图、点击、输入等工具来控制桌面。

**保留 server 名称**: `computer-use`  
所有 Computer Use 工具以 `mcp__computer-use__*` 命名，用于权限识别、日志和 UI 文案。

## 配置方式

在 `~/.cc-rust/settings.json` (全局) 或 `.cc-rust/settings.json` (项目) 中添加 `mcpServers.computer-use`。

### Windows

```json
{
  "mcpServers": {
    "computer-use": {
      "command": "npx",
      "args": ["-y", "@anthropic-ai/computer-use-mcp-server"],
      "env": {
        "DISPLAY_WIDTH": "1920",
        "DISPLAY_HEIGHT": "1080"
      }
    }
  }
}
```

### macOS

```json
{
  "mcpServers": {
    "computer-use": {
      "command": "npx",
      "args": ["-y", "@anthropic-ai/computer-use-mcp-server"],
      "env": {
        "DISPLAY_WIDTH": "1920",
        "DISPLAY_HEIGHT": "1080"
      }
    }
  }
}
```

> macOS 首次使用需授权 **辅助功能** (Accessibility) 和 **屏幕录制** (Screen Recording) 权限。

### Linux (X11)

```json
{
  "mcpServers": {
    "computer-use": {
      "command": "npx",
      "args": ["-y", "@anthropic-ai/computer-use-mcp-server"],
      "env": {
        "DISPLAY": ":0",
        "DISPLAY_WIDTH": "1920",
        "DISPLAY_HEIGHT": "1080"
      }
    }
  }
}
```

> 需安装: `xdotool`, `scrot` (或 `grim` for Wayland)

## 可用工具

配置完成后，以下工具自动注册:

| 工具名 | 描述 | 风险等级 |
|--------|------|----------|
| `mcp__computer-use__screenshot` | 截取当前屏幕 | 中 |
| `mcp__computer-use__left_click` | 左键点击指定坐标 | 高 |
| `mcp__computer-use__right_click` | 右键点击指定坐标 | 高 |
| `mcp__computer-use__double_click` | 双击指定坐标 | 高 |
| `mcp__computer-use__type_text` | 输入文本 | 高 |
| `mcp__computer-use__key` | 按键/快捷键 | 高 |
| `mcp__computer-use__scroll` | 滚动鼠标 | 中 |
| `mcp__computer-use__cursor_position` | 获取当前光标位置 | 低 |

## 权限配置

### 允许所有 Computer Use 工具 (谨慎)

```json
{
  "permissions": {
    "allow": [
      "mcp__computer-use__screenshot",
      "mcp__computer-use__left_click",
      "mcp__computer-use__type_text",
      "mcp__computer-use__key",
      "mcp__computer-use__scroll"
    ]
  }
}
```

### 只允许截图 (推荐起步)

```json
{
  "permissions": {
    "allow": ["mcp__computer-use__screenshot"]
  }
}
```

## 验证

启动 cc-rust 后，在对话中输入:

```
请截取一张当前屏幕的截图
```

如果配置正确，模型将调用 `mcp__computer-use__screenshot`，返回真实图片并进行分析。

## 故障排查

1. **MCP server 连接失败**: 检查 `npx` 是否在 PATH 中，Node.js >= 18
2. **截图返回空**: 检查显示器分辨率配置 (`DISPLAY_WIDTH`/`DISPLAY_HEIGHT`)
3. **权限被拒**: 检查 `~/.cc-rust/settings.json` 中的 permissions 配置
4. **macOS 无响应**: 在 系统偏好设置 → 隐私与安全性 中授权终端应用
