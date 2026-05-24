# Browser MCP 配置指南

## 概述

cc-rust 通过外置 MCP (Model Context Protocol) 服务器实现浏览器自动化能力。
本仓库 **不直接实现 Chrome 集成**，而是复用已有的 MCP 通道接入第三方/自托管的
browser MCP 服务（例如 `mcp-chrome`、`@playwright/mcp`、`browser-use-mcp` 等）。

cc-rust 会在 MCP 发现阶段识别这些工具并为它们提供：

- **专用系统提示** — 当发现任何 browser MCP 工具时，在系统提示中注入一段
  `# Browser Automation` 说明，让模型了解浏览器能力以及使用守则。
- **分类权限提示** — 浏览器操作按 navigation / read / write / upload / js / observability
  分类，授权对话框显示 “Allow navigating the browser?” 而不是
  “Allow tool 'mcp__chrome__navigate'?”。
- **富化结果预览** — 导航、页面读取、console/network、screenshot 等结果会在
  UI 中以精简形式显示（例如 `[navigation] navigated → https://example.com`）。
- **`/mcp list` 标签** — browser 服务会带 `[browser]` 标记。

## 标识方式

只要满足任一条件，cc-rust 就会把服务视为 browser MCP：

1. **配置显式标记**：在服务条目中设置 `"browserMcp": true`。这是最可靠的方式，
   因为即使该服务暴露的工具不是 cc-rust 内建清单里的名字，也会被识别。
2. **工具名启发式**：服务暴露了任意一个已知的浏览器动作基名（详见下方 "已识别的工具基名"）。
   适合绝大多数主流 browser MCP 实现，**无需额外配置即可工作**。

两条路径互不排斥；配置标记永远优先。

## 最小示例（单会话、手动配置）

在 `~/.cc-rust/settings.json` 或项目级 `.cc-rust/settings.json` 的
`mcpServers` 中添加一个条目。

### 场景 A：使用 npm 上的 browser MCP（启发式即可）

```json
{
  "mcpServers": {
    "chrome": {
      "command": "npx",
      "args": ["-y", "mcp-chrome"]
    }
  }
}
```

`mcp-chrome` 暴露 `navigate`、`get_page_text`、`take_screenshot` 等工具；
启发式识别会自动把它标记为 browser MCP。

### 场景 B：自托管 stdio 服务（显式标记）

当你使用自己写的 MCP 服务、或者工具名称不在默认清单里时，加上 `"browserMcp": true`：

```json
{
  "mcpServers": {
    "my-browser": {
      "command": "node",
      "args": ["./tools/browser-mcp/dist/index.js"],
      "env": {
        "HEADLESS": "1"
      },
      "browserMcp": true
    }
  }
}
```

### 场景 C：SSE 传输（远端 browser MCP）

```json
{
  "mcpServers": {
    "remote-browser": {
      "type": "sse",
      "url": "http://127.0.0.1:9222/mcp/browser",
      "headers": {
        "Authorization": "Bearer ${BROWSER_MCP_TOKEN}"
      },
      "browserMcp": true
    }
  }
}
```

## 快速验证

启动 cc-rust 后：

```
/mcp list
```

应看到类似：

```
Discovered MCP servers (1; 1 browser):

  chrome [browser] -- transport: stdio -- command: npx -y mcp-chrome

Browser-tagged servers expose browser-automation tools (navigate, read_page, click, …).
See docs/reference/browser-mcp-config.md.
```

随后让模型尝试 `tabs_context → navigate → read_page/get_page_text` 这一条读链路，
系统会在每次浏览器工具调用时弹出分类授权对话。

## 已识别的工具基名

cc-rust 把下面的基名识别为 browser MCP（按类别）：

- **导航/标签页**：`navigate`, `navigate_page`, `goto`, `tabs_create`, `tabs_close`,
  `tabs_context`, `new_page`, `close_page`, `list_pages`, `switch_browser`, `select_page`
- **页面读取**：`read_page`, `get_page_text`, `get_page`, `take_snapshot`, `snapshot`
- **交互/写入**：`click`, `double_click`, `hover`, `drag`, `press_key`, `type_text`,
  `fill`, `fill_form`, `form_input`, `select`, `resize_page`, `emulate`, `handle_dialog`
- **文件上传**：`upload_file`, `file_upload`
- **JS 执行**：`evaluate_script`, `javascript_tool`, `evaluate`
- **Console / Network**：`get_console_message`, `list_console_messages`,
  `read_console_messages`, `get_network_request`, `list_network_requests`,
  `read_network_requests`
- **截图/视觉**：`take_screenshot`, `screenshot`
- **其他**：`wait_for`, `find`, `resize_window`

完整列表见 `crates/cc-browser/src/detection.rs` 的 `BROWSER_TOOL_BASENAMES`。

如果你的 browser MCP 服务使用了不在上面的工具名，**设置 `"browserMcp": true` 即可**
让 cc-rust 把它当作 browser MCP 处理（权限对话会退化为通用的 browser 风格提示）。

## 非目标（MVP 范围）

- ❌ 不把第一方 Chrome 扩展或 native host 作为本指南的发布承诺
- ❌ 不要求 `--chrome` 命令行或 `/chrome` 斜杠命令；这些属于后续 native-host 路线
- ❌ 不做 session 之间共享浏览器状态（由 browser MCP 服务自行负责）
- ❌ 不做完整的浏览器权限模型（仅粗粒度分类授权）

## 与 Computer Use 的区别

|           | Computer Use            | Browser MCP                   |
|-----------|-------------------------|-------------------------------|
| 目标      | 操作整个桌面            | 仅操作浏览器                  |
| 保留 server 名 | `computer-use`     | 任意（按标志/工具名识别）    |
| 工具基名 | `screenshot`, `left_click`, `type_text`, … | `navigate`, `read_page`, `click`, … |
| 权限分类 | screenshot / click / input | navigation / read / write / upload / js / observability |
| 提示词   | `# Computer Use`        | `# Browser Automation`        |

两套系统独立工作，可以同时启用。
