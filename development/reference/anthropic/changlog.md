[2026/3/27 18:01] Claude Code Changelog: Claude Code 2.1.86 Released

Key Updates:
• Added X-Claude-Code-Session-Id header to allow proxies to aggregate requests by session
• Fixed --resume failing on sessions created before v2.1.85 due to tool result errors
• Fixed Write/Edit/Read failures for files outside the project root (e.g., CLAUDE.md)
• Fixed performance issues and potential config corruption caused by excessive disk writes
• Fixed "Permission denied" errors for marketplace plugin scripts on macOS/Linux
• Reduced token usage by optimizing @ file mentions and the Read tool's output format
• Improved prompt cache hit rates for Bedrock, Vertex, and Foundry by stabilizing tool descriptions
• Fixed memory leaks and out-of-memory crashes during long sessions or feedback exports

更新要点：
• 新增 X-Claude-Code-Session-Id 请求头，便于代理按会话聚合请求
• 修复了 v2.1.85 之前创建的会话在使用 --resume 时因工具结果错误而失败的问题
• 修复了在配置规则时无法读写项目根目录外文件（如 CLAUDE.md）的问题
• 修复了因频繁写入配置导致的性能下降及 Windows 平台下的配置损坏风险
• 修复了 macOS/Linux 系统上市场插件脚本运行时的“权限拒绝”错误
• 通过优化 @ 文件引用和 Read 工具的输出格式，有效降低了 Token 消耗
• 通过移除工具描述中的动态内容，提升了 Bedrock、Vertex 和 Foundry 用户的缓存命中率
• 修复了长会话或导出反馈时可能出现的内存泄漏及内存溢出崩溃问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-2186-Release-Notes-03-27)
[2026/3/28 22:30] Claude Code Changelog: Claude Code 2.1.87 Released

• Fixed messages in Cowork Dispatch not getting delivered

• 修复了 Cowork Dispatch 中的消息无法送达的问题
[2026/3/30 20:01] Claude Code Changelog: Claude Code 2.1.88 Released

Key Updates:
• Added CLAUDE_CODE_NO_FLICKER=1 for flicker-free rendering with virtualized scrollback
• Added PermissionDenied hook allowing models to retry after auto-mode classifier denials
• Fixed prompt cache misses and redundant CLAUDE.md injections in long sessions
• Fixed Edit/Write tools doubling CRLF on Windows and stripping Markdown hard line breaks
• Fixed StructuredOutput schema cache bug that caused high failure rates in multi-schema workflows
• Fixed CJK and emoji characters being dropped from prompt history at 4KB boundaries
• Thinking summaries are now disabled by default in interactive sessions
• Improved PowerShell tool with version-specific syntax guidance and /env support

更新要点：
• 新增 CLAUDE_CODE_NO_FLICKER=1 环境变量，支持无闪烁渲染与虚拟滚动回溯
• 新增 PermissionDenied 钩子，允许模型在自动模式拦截后尝试重试
• 修复了长会话中提示词缓存失效以及 CLAUDE.md 文件被重复注入的问题
• 修复了 Windows 平台上编辑工具导致换行符重复及 Markdown 硬换行丢失的问题
• 修复了 StructuredOutput 架构缓存错误导致的跨工作流高失败率问题
• 修复了提示词历史记录中 CJK 字符和表情符号在 4KB 边界处被丢弃的问题
• 交互模式下默认不再生成思考摘要（Thinking summaries）
• 改进了 PowerShell 工具，支持 /env 变量并提供针对不同版本的语法引导

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-2188-Release-Notes-03-31)
[2026/3/31 21:31] Claude Code Changelog: Claude Code 2.1.89 Released

Key Updates:
• Added "defer" decision to PreToolUse hooks, allowing headless sessions to pause and resume tool calls
• Introduced CLAUDE_CODE_NO_FLICKER=1 for flicker-free rendering with virtualized scrollback
• Added PermissionDenied hook and retry logic for auto mode classifier denials
• Fixed Edit/Write tools doubling CRLF on Windows and stripping Markdown line breaks
• Fixed a StructuredOutput cache bug that caused a ~50% failure rate with multiple schemas
• Fixed prompt history data loss for CJK or emoji characters on 4KB boundaries
• Improved Bash tool to warn when linter/formatter commands modify previously read files
• Changed thinking summaries to be disabled by default in interactive sessions

更新要点：
• PreToolUse 钩子新增 "defer" 选项，支持无头模式暂停并在稍后恢复工具调用
• 引入 CLAUDE_CODE_NO_FLICKER=1 环境变量，实现无闪烁的虚拟滚动渲染
• 新增 PermissionDenied 钩子及重试逻辑，用于处理自动模式下的分类器拒绝
• 修复了 Windows 平台上 Edit/Write 工具导致 CRLF 换行符翻倍及 Markdown 换行丢失的问题
• 修复了 StructuredOutput 架构缓存 Bug，该问题曾导致多架构使用时约 50% 的失败率
• 修复了提示词历史记录中 CJK 字符或表情符号在 4KB 边界处被静默丢弃的问题
• 改进了 Bash 工具，当格式化或 Linter 命令修改了已读取的文件时会发出警告
• 更改了交互模式下的默认设置，现在默认不再生成思维摘要 (Thinking Summaries)

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-2189-Release-Notes-04-01)
[2026/4/1 20:01] Claude Code Changelog: Claude Code 2.1.90 Released

• Added /powerup — interactive lessons teaching Claude Code features with animated demos
• Added CLAUDE_CODE_PLUGIN_KEEP_MARKETPLACE_ON_FAILURE env var to keep the existing marketplace cache when git pull fails, useful in offline environments
• Added .husky to protected directories (acceptEdits mode)
• Fixed an infinite loop where the rate-limit options dialog would repeatedly auto-open after hitting your usage limit, eventually crashing the session
• Fixed --resume causing a full prompt-cache miss on the first request for users with deferred tools, MCP servers, or custom agents (regression since v2.1.69)
• Fixed Edit/Write failing with "File content has changed" when a PostToolUse format-on-save hook rewrites the file between consecutive edits
• Fixed PreToolUse hooks that emit JSON to stdout and exit with code 2 not correctly blocking the tool call
• Fixed collapsed search/read summary badge appearing multiple times in fullscreen scrollback when a CLAUDE.md file auto-loads during a tool call
• Fixed auto mode not respecting explicit user boundaries ("don't push", "wait for X before Y") even when the action would otherwise be allowed
• Fixed click-to-expand hover text being nearly invisible on light terminal themes
• Fixed UI crash when malformed tool input reached the permission dialog
• Fixed headers disappearing when scrolling /model, /config, and other selection screens
• Hardened PowerShell tool permission checks: fixed trailing & background job bypass, -ErrorAction Break debugger hang, archive-extraction TOCTOU, and parse-fail fallback deny-rule degradation
• Improved performance: eliminated per-turn JSON.stringify of MCP tool schemas on cache-key lookup
• Improved performance: SSE transport now handles large streamed frames in linear time (was quadratic)
• Improved performance: SDK sessions with long conversations no longer slow down quadratically on transcript writes
• Improved /resume all-projects view to load project sessions in parallel, improving load times for users with many projects
• Changed --resume picker to no longer show sessions created by claude -p or SDK invocations
• Removed Get-DnsClientCache and ipconfig /displaydns from auto-allow (DNS cache privacy)

• 新增 /powerup — 通过动画演示教学 Claude Code 功能的交互式课程
• 新增 CLAUDE_CODE_PLUGIN_KEEP_MARKETPLACE_ON_FAILURE 环境变量，当 git pull 失败时保留现有的 marketplace 缓存，适用于离线环境
• 将 .husky 添加到受保护目录（acceptEdits 模式）
• 修复了在达到使用限制后，速率限制选项对话框会重复自动打开，最终导致会话崩溃的无限循环问题
• 修复了对于使用延迟工具、MCP 服务器或自定义 Agent 的用户，--resume 导致第一次请求时出现完整的 prompt cache 未命中问题（自 v2.1.69 以来的回归缺陷）
• 修复了当 PostToolUse 保存时格式化 Hook 在连续编辑之间重写文件时，Edit/Write 失败并提示 "File content has changed" 的问题
• 修复了向 stdout 输出 JSON 并以代码 2 导出的 PreToolUse Hook 未能正确拦截 Tool Call 的问题
• 修复了在 Tool Call 期间自动加载 CLAUDE.md 文件时，折叠的搜索/读取摘要徽章在全屏滚动回溯中多次出现的问题
• 修复了即使在操作被允许的情况下，自动模式也不遵守明确的用户边界（如 "don't push"、"wait for X before Y"）的问题
• 修复了在浅色终端主题上，点击展开的悬停文本几乎不可见的问题
• 修复了当格式错误的工具输入到达 Permission 对话框时导致的 UI 崩溃
• 修复了滚动 /model、/config 和其他选择界面时标题消失的问题
• 加固了 PowerShell 工具的 Permission 检查：修复了末尾 & 后台作业绕过、-ErrorAction Break 调试器挂起、归档解压 TOCTOU 以及解析失败回退拒绝规则降级的问题
• 提升性能：在 cache-key 查找时消除了每轮对 MCP 工具 Schema 的 JSON.stringify 操作
• 提升性能：SSE 传输现在以线性时间处理大型 Streaming 帧（原为二次方时间）
• 提升性能：具有长对话的 SDK 会话在写入 Transcript Mode 时不再呈二次方减速
• 改进了 /resume 全项目视图，并行加载项目会话，缩短了拥有多个项目用户的加载时间
• 变更了 --resume 选择器，不再显示由 claude -p 或 SDK 调用创建的会话
• 从自动允许列表中移除了 Get-DnsClientCache 和 ipconfig /displaydns（DNS 缓存隐私）
[2026/4/2 20:11] Claude Code Changelog: Claude Code 2.1.91 Released

• Added MCP tool result persistence override via _meta["anthropic/maxResultSizeChars"] annotation (up to 500K), allowing larger results like DB schemas to pass through without truncation
• Added disableSkillShellExecution setting to disable inline shell execution in skills, custom slash commands, and plugin commands
• Added support for multi-line prompts in claude-cli://open?q= deep links (encoded newlines %0A no longer rejected)
• Plugins can now ship executables under bin/ and invoke them as bare commands from the Bash tool
• Fixed transcript chain breaks on --resume that could lose conversation history when async transcript writes fail silently
• Fixed cmd+delete not deleting to start of line on iTerm2, kitty, WezTerm, Ghostty, and Windows Terminal
• Fixed plan mode in remote sessions losing track of the plan file after a container restart, which caused permission prompts on plan edits and an empty plan-approval modal
• Fixed JSON schema validation for permissions.defaultMode: "auto" in settings.json
• Fixed Windows version cleanup not protecting the active version's rollback copy
• /feedback now explains why it's unavailable instead of disappearing from the slash menu
• Improved /claude-api skill guidance for agent design patterns including tool surface decisions, context management, and caching strategy
• Improved performance: faster stripAnsi on Bun by routing through Bun.stripANSI
• Edit tool now uses shorter old_string anchors, reducing output tokens

• 新增通过 _meta["anthropic/maxResultSizeChars"] 注解覆盖 MCP 工具结果持久化的功能（最高支持 500K），允许像 DB schemas 这样较大的结果通过而不会被截断
• 新增 disableSkillShellExecution 设置，用于禁用 Skill、自定义斜杠命令和 Plugin 命令中的内联 shell 执行
• 新增对 claude-cli://open?q= deep links 中多行 Prompt 的支持（不再拒绝编码后的换行符 %0A）
• Plugin 现在可以在 bin/ 下附带可执行文件，并从 Bash Tool 中作为基础命令调用它们
• 修复了使用 /resume 时 Transcript 链断裂的问题，该问题曾导致异步 Transcript 写入静默失败时丢失对话历史
• 修复了在 iTerm2, kitty, WezTerm, Ghostty 和 Windows Terminal 上 cmd+delete 无法删除到行首的问题
• 修复了远程会话中 Plan Mode 在容器重启后丢失对 plan 文件追踪的问题，该问题曾导致编辑 plan 时出现 Permission 提示以及空的 plan-approval 模态框
• 修复了 settings.json 中 permissions.defaultMode: "auto" 的 JSON schema 校验问题
• 修复了 Windows 版本清理时未保护当前激活版本的回归副本的问题
• /feedback 现在会解释其不可用的原因，而不是直接从斜杠菜单中消失
• 改进了 /claude-api Skill 对 Agent 设计模式的引导，包括工具表面决策、上下文管理和缓存策略
• 提升了性能：通过路由至 Bun.stripANSI 使 Bun 上的 stripAnsi 速度更快
• 编辑工具现在使用更短的 old_string 锚点，减少了输出 Token 数
[2026/4/3 21:01] Claude Code Changelog: Claude Code 2.1.92 Released

Key Updates:
• Added forceRemoteSettingsRefresh policy to block startup until remote settings are fetched
• New interactive Bedrock setup wizard for AWS authentication and model configuration
• Added per-model and cache-hit cost breakdown to the /cost command
• Improved Write tool performance with 60% faster diff computation for large files
• Fixed subagent spawning failures caused by tmux window renumbering or killing
• Fixed tool input validation errors when streaming JSON-encoded array/object fields
• Removed /tag and /vim commands (Vim mode is now toggled via /config)

更新要点：
• 新增 forceRemoteSettingsRefresh 策略，强制在启动前同步远程设置
• 新增 Bedrock 交互式设置向导，简化 AWS 认证与模型配置流程
• /cost 命令现支持按模型和缓存命中情况显示详细费用明细
• 优化 Write 工具性能，大文件的差异计算速度提升 60%
• 修复了因 tmux 窗口重排或关闭导致子代理（subagent）启动失败的问题
• 修复了流式传输 JSON 编码字段时工具输入验证失败的问题
• 移除了 /tag 和 /vim 命令（Vim 模式现通过 /config 切换）

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-2192-Release-Notes-04-04)
[2026/4/7 17:31] Claude Code Changelog: Claude Code 2.1.94 Released

Key Updates:
• Added support for Amazon Bedrock powered by Mantle via CLAUDE_CODE_USE_MANTLE=1
• Increased default reasoning effort level from medium to high for most paid and API users
• Fixed agents appearing stuck after 429 rate-limit errors; errors now surface immediately
• Improved --resume to directly open sessions from different worktrees of the same repo
• Fixed corruption of CJK and multibyte text caused by UTF-8 sequence splitting in streams
• Fixed macOS login failures caused by locked keychains with new diagnostic support in claude doctor
• Plugin skills now use stable frontmatter names instead of directory names for invocations
• Resolved terminal rendering issues including ghost lines and duplicate hyperlinks in tmux

更新要点：
• 新增通过 Mantle 支持 Amazon Bedrock，可通过环境变量 CLAUDE_CODE_USE_MANTLE=1 开启
• 调高了 API 及企业版用户的默认思考强度（Effort Level），从“中”提升至“高”
• 修复了代理在遇到 429 频率限制错误时卡住的问题，现在会立即显示错误提示
• 优化了 --resume 功能，支持直接恢复同仓库下其他工作区（worktrees）的会话
• 修复了流式传输中因 UTF-8 序列被截断导致的中日韩（CJK）多字节字符乱码问题
• 修复了 macOS 因钥匙串锁定导致登录失败的问题，并可通过 claude doctor 诊断修复
• 插件技能现在使用 frontmatter 中定义的名称而非目录名，确保调用名称更稳定
• 解决了终端渲染问题，包括 tmux 环境下的重复超链接和滚动时的残影行

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-2194-Release-Notes-04-07)
[2026/4/8 1:00] Claude Code Changelog: Claude Code 2.1.96 Released

• Fixed Bedrock requests failing with 403 "Authorization header is missing" when using AWS_BEARER_TOKEN_BEDROCK or CLAUDE_CODE_SKIP_BEDROCK_AUTH (regression in 2.1.94)

• 修复了在使用 AWS_BEARER_TOKEN_BEDROCK 或 CLAUDE_CODE_SKIP_BEDROCK_AUTH 时，Bedrock 请求因 403 "Authorization header is missing" 而失败的问题（2.1.94 版本中的回归错误）
[2026/4/8 18:01] Claude Code Changelog: Claude Code 2.1.97 Released

Key Updates:
• Added Focus View (Ctrl+O) in NO_FLICKER mode for a cleaner view of prompts, diffstats, and responses
• Enhanced Bash tool security with tighter permission checks and reduced false prompts for common commands
• Improved auto-approval logic for filesystem commands and sandbox network access in various modes
• Fixed MCP memory leaks and OAuth token refresh issues for better stability with external tools
• Fixed several /resume issues, including uneditable sessions, lost mid-turn inputs, and diff rendering
• Improved CJK input support by allowing slash commands and @-mentions to trigger after punctuation
• Optimized session transcript size and memory usage by capping file copies and skipping empty entries

更新要点：
• NO_FLICKER 模式新增焦点视图 (Ctrl+O)，可清晰查看提示词、差异统计和响应
• 增强 Bash 工具安全性，收紧权限检查并减少常用命令的误报提示
• 优化了多种模式下文件系统命令和沙箱网络访问的自动审批逻辑
• 修复了 MCP 内存泄漏和 OAuth 令牌刷新问题，提升外部工具稳定性
• 修复了 /resume 恢复功能的多项错误，包括会话不可编辑、输入丢失和差异显示问题
• 改进对中日韩文字的支持，允许在标点符号后直接触发斜杠命令和 @ 提及
• 通过限制文件副本存储和跳过空条目，优化了会话副本的大小和内存占用

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-2197-Release-Notes-04-08)
[2026/4/9 15:31] Claude Code Changelog: Claude Code 2.1.98 Released

Key Updates:
• Added an interactive Google Vertex AI setup wizard for easier GCP authentication and configuration
• Introduced a Monitor tool for streaming events from background scripts and subprocess sandboxing on Linux
• Added CLAUDE_CODE_PERFORCE_MODE to prevent silent overwrites of read-only files in Perforce environments
• Fixed a critical Bash tool security vulnerability where escaped flags could bypass permission checks
• Improved rate-limit handling with mandatory exponential backoff to prevent rapid retry exhaustion
• Enhanced the /resume and /agents commands with better filtering, project metadata, and a new tabbed layout
• Fixed various terminal UI issues, including character dropping in xterm/VS Code and macOS text replacement bugs

更新要点：
• 新增 Google Vertex AI 交互式配置向导，简化 GCP 认证与项目配置流程
• 引入用于流式传输后台脚本事件的 Monitor 工具，并为 Linux 增加了子进程沙箱隔离
• 新增 CLAUDE_CODE_PERFORCE_MODE 环境变量，防止在 Perforce 环境下静默覆盖只读文件
• 修复了 Bash 工具的一个关键安全漏洞，防止通过转义字符绕过权限检查执行任意代码
• 优化了 429 速率限制处理机制，引入强制指数退避算法以避免快速耗尽重试次数
• 改进了 /resume 和 /agents 命令，增加了项目元数据过滤及新的标签页布局
• 修复了多项终端 UI 问题，包括 xterm/VS Code 中的字符丢失以及 macOS 下的文本替换错误

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-2198-Release-Notes-04-09)
[2026/4/10 15:31] Claude Code Changelog: Claude Code 2.1.101 Released

Key Updates:
• Added /team-onboarding command to generate teammate ramp-up guides from local usage
• Added default support for OS CA certificate stores to simplify enterprise TLS proxy setup
• Fixed a command injection vulnerability in the POSIX which fallback for LSP detection
• Fixed a hardcoded 5-minute timeout that previously aborted slow backend or local LLM requests
• Improved session management: --resume now supports custom names and fixes context loss issues
• Improved rate-limit and refusal messages to include specific reset times and API explanations
• Fixed memory leaks in long sessions and excessive disk writes when using the /btw command
• Fixed subagent permission issues and MCP tool inheritance in isolated worktrees

更新要点：
• 新增 /team-onboarding 命令，可根据本地使用记录生成团队成员上手指南
• 默认信任操作系统 CA 证书库，简化了企业级 TLS 代理环境下的配置
• 修复了 LSP 二进制检测中 POSIX which 回退方案存在的命令注入漏洞
• 修复了硬编码的 5 分钟超时限制，解决了本地 LLM 或长思考过程被意外中断的问题
• 改进会话管理：--resume 现在支持自定义名称，并修复了大对话中的上下文丢失问题
• 优化了速率限制和拒绝访问的错误提示，现在会显示具体的重置时间及 API 解释
• 修复了长会话中的内存泄漏问题，以及使用 /btw 命令时频繁写入磁盘的 Bug
• 修复了子代理在隔离工作区中的权限错误及 MCP 工具继承失效的问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21101-Release-Notes-04-10)
[2026/4/13 18:00] Claude Code Changelog: Claude Code 2.1.105 Released

Key Updates:
• Added path parameter to EnterWorktree for switching to existing repository worktrees
• Added background monitor support for plugins via the monitors manifest key
• Improved stalled API handling by aborting silent streams after 5 minutes and retrying
• Improved WebFetch by stripping CSS and scripts to prevent token budget exhaustion
• Improved /doctor with status icons and an 'f' shortcut for automated fixes
• Fixed images in queued messages being dropped while Claude is processing
• Fixed various UI issues including blank screens on line wraps and garbled bash output
• Fixed 429 rate-limit errors to show clean messages instead of raw JSON dumps

更新要点：
• EnterWorktree 工具新增 path 参数，支持切换至现有的工作树
• 插件新增后台监控支持，可通过清单文件中的 monitors 键自动启动
• 优化了 API 流超时处理，无数据 5 分钟后将自动中止并尝试非流式重试
• 改进了 WebFetch 功能，自动剔除 CSS 和脚本以节省上下文 Token 配额
• 优化了 /doctor 布局并支持状态图标，按 'f' 键可让 Claude 自动修复问题
• 修复了在 Claude 处理任务时发送的消息中图片附件丢失的问题
• 修复了输入框换行导致屏幕空白及 Bash 输出乱码等多个 UI 交互问题
• 修复了 429 速率限制错误显示原始 JSON 的问题，现提供更清晰的错误提示

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21105-Release-Notes-04-13)
[2026/4/14 2:30] Claude Code Changelog: Claude Code 2.1.107 Released

• Show thinking hints sooner during long operations

• 在长时间操作期间更早地显示 thinking hints
[2026/4/14 15:30] Claude Code Changelog: Claude Code 2.1.108 Released

Key Updates:
• Added new recap feature to provide context when returning to a session via the /recap command
• Introduced ENABLE_PROMPT_CACHING_1H to opt into 1-hour prompt cache TTL across multiple providers
• The model can now autonomously discover and invoke built-in slash commands like /init and /review
• Improved /resume picker to default to current directory sessions with a new Ctrl+A toggle for all projects
• Improved error handling for rate limits and server errors with direct links to status pages
• Reduced memory footprint by loading language grammars for syntax highlighting on demand
• Fixed issues with diacritical marks being dropped and paste functionality in the login prompt

更新要点：
• 新增回顾 (recap) 功能，可通过 /recap 命令在返回会话时提供上下文摘要
• 引入 ENABLE_PROMPT_CACHING_1H 环境变量，支持在多个提供商上开启 1 小时缓存时长
• 模型现在可以通过 Skill 工具自动发现并调用 /init 和 /review 等内置斜杠命令
• 优化了 /resume 会话选择器，默认显示当前目录会话并支持通过 Ctrl+A 查看全部
• 改进了错误处理机制，能够区分频率限制与套餐限制，并提供状态页链接
• 通过按需加载语法高亮资源，降低了文件读取和编辑时的内存占用
• 修复了响应中变音符号丢失以及登录界面无法粘贴代码的问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21108-Release-Notes-04-14)
[2026/4/15 0:30] Claude Code Changelog: Claude Code 2.1.109 Released

• Improved the extended-thinking indicator with a rotating progress hint

• 改进了带有旋转进度提示的 extended-thinking 指示器
[2026/4/15 18:30] Claude Code Changelog: Claude Code 2.1.110 Released

Key Updates:
• Added /tui command for flicker-free fullscreen rendering within the same conversation
• Claude can now send mobile push notifications when Remote Control is enabled
• Added /focus command to toggle focus view, separating it from the Ctrl+O transcript toggle
• Remote Control now supports /context, /exit, and /reload-plugins commands
• Improved /plugin management with better sorting, favorites, and dependency handling
• Fixed MCP tool calls hanging indefinitely when server connections drop mid-response
• Fixed high CPU usage in fullscreen mode when selecting text during tool execution
• Enhanced security for "Open in editor" actions to prevent command injection

更新要点：
• 新增 /tui 命令，支持在同一次对话中切换至无闪烁的全屏渲染模式
• 开启远程控制后，Claude 现在可以向移动端发送推送通知
• 新增 /focus 命令以切换焦点视图，与 Ctrl+O 的逐字稿切换功能分离
• 远程控制端现已支持 /context、/exit 和 /reload-plugins 等命令
• 优化了 /plugin 插件管理，支持排序、收藏及自动安装依赖项
• 修复了 MCP 工具调用在服务器连接中断时可能导致无限挂起的问题
• 修复了全屏模式下运行工具时选中文本导致 CPU 占用过高的问题
• 加固了“在编辑器中打开”操作的安全防护，防止恶意文件名的命令注入

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21110-Release-Notes-04-16)
[2026/4/16 11:31] Claude Code Changelog: Claude Code 2.1.111 Released

Key Updates:
• Added support for Claude Opus 4.7 with a new xhigh effort level and Auto mode for Max subscribers
• Introduced /ultrareview for multi-agent parallel code reviews of local branches or GitHub PRs
• Added /less-permission-prompts to automatically allowlist common read-only Bash and MCP commands
• New interactive /effort slider and "Auto (match terminal)" theme option
• Progressive rollout of PowerShell tool support for Windows users
• Improved terminal UX with typo suggestions for subcommands and better handling of long pastes
• Fixed terminal display tearing in iTerm2 + tmux and optimized file suggestion performance

更新要点：
• 支持 Claude Opus 4.7 模型，新增 xhigh 思考强度及面向 Max 订阅者的自动模式
• 新增 /ultrareview 功能，支持通过多智能体并行分析本地分支或 GitHub PR 代码
• 新增 /less-permission-prompts 技能，可自动将常用的只读 Bash 和 MCP 指令加入白名单
• /effort 指令现支持交互式滑块调节，并新增“跟随终端”主题选项
• Windows 平台逐步推出 PowerShell 工具支持
• 优化终端体验，包括子命令拼写纠错建议以及长文本粘贴的显示优化
• 修复了 iTerm2 + tmux 环境下的显示撕裂问题，并优化了文件建议的扫描性能

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21111-Release-Notes-04-16)
[2026/4/16 16:30] Claude Code Changelog: Claude Code 2.1.112 Released

• Fixed "claude-opus-4-7 is temporarily unavailable" for auto mode

• 修复了 auto mode 下 "claude-opus-4-7 is temporarily unavailable" 的问题
[2026/4/17 16:00] Claude Code Changelog: Claude Code 2.1.113 Released

Key Updates:
• CLI now spawns a native binary instead of bundled JavaScript for improved performance
• Added sandbox.network.deniedDomains to block specific domains within allowed wildcards
• Enhanced security by preventing find -exec/-delete auto-approval and improving exec wrapper detection
• Improved /ultrareview with parallelized checks, diffstat previews, and faster launch times
• Fixed a security vulnerability where dangerouslyDisableSandbox could bypass permission prompts
• Fixed MCP concurrent-call timeouts and subagent transcript visibility in Remote Control sessions
• Subagents now fail with a clear error after 10 minutes of stalling instead of hanging

更新要点：
• CLI 现在运行原生二进制文件而非捆绑的 JavaScript，以提升性能
• 新增 sandbox.network.deniedDomains 设置，支持在通配符允许范围内禁用特定域名
• 增强安全性：禁止自动批准 find -exec/-delete，并改进了对 exec 包装命令的检测
• 优化 /ultrareview 功能，支持并行检查、diffstat 预览并缩短启动时间
• 修复了 dangerouslyDisableSandbox 可能绕过权限提示直接运行沙箱外命令的安全漏洞
• 修复了 MCP 并发调用超时问题以及远程控制会话中子代理记录的显示问题
• 子代理在停滞 10 分钟后将报错退出，不再无响应挂起

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21113-Release-Notes-04-17)
[2026/4/17 22:00] Claude Code Changelog: Claude Code 2.1.114 Released

• Fixed a crash in the permission dialog when an agent teams teammate requested tool permission

• 修复了当 Agent Teams 队友请求工具 Permission 时，权限对话框发生的崩溃问题
[2026/4/20 18:30] Claude Code Changelog: Claude Code 2.1.116 Released

Key Updates:
• Significantly improved /resume speed for large sessions and fixed empty conversation errors
• Faster MCP startup by deferring resource template loading until the first mention
• Enhanced terminal scrolling and keyboard shortcut compatibility across major terminal emulators
• Thinking spinner now displays inline progress status instead of a separate hint row
• Security: Sandbox auto-allow now enforces safety checks for dangerous commands like rm on system directories
• Fixed terminal rendering issues including Indic script alignment and scrollback duplication
• Usage tab now displays 5-hour and weekly stats immediately with improved reliability

更新要点：
• 大幅提升大型会话的 /resume 加载速度，并修复了加载错误导致显示空白的问题
• 通过延迟加载资源模板，加快了配置多个 stdio 服务器时的 MCP 启动速度
• 优化了主流终端模拟器的滚动平滑度，并修复了 Kitty 协议下的快捷键兼容性
• “思考中”状态现在直接在行内显示进度描述，取代了原有的独立提示行
• 安全增强：沙箱自动允许机制现在会对针对系统目录的 rm 等危险指令执行安全检查
• 修复了终端渲染问题，包括印地语脚本对齐异常及滚动时的内容重复问题
• 设置中的用量标签页现在能立即显示 5 小时和每周统计，且更加稳定

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21116-Release-Notes-04-20)
[2026/4/21 20:31] Claude Code Changelog: Claude Code 2.1.117 Released

Key Updates:
• Improved /model persistence and visibility of project-pinned vs. user-selected models
• Enhanced /resume command to summarize large, stale sessions before re-reading
• Optimized startup speed via concurrent connection to local and claude.ai MCP servers
• Upgraded plugin system to auto-resolve and install missing dependencies
• Replaced Glob/Grep tools with embedded bfs/ugrep on macOS/Linux for faster searches
• Increased default effort level to high for Pro/Max subscribers on supported models
• Fixed OAuth session expiration issues by implementing reactive token refreshing
• Fixed context window calculation for Opus 4.7 to prevent premature autocompaction

更新要点：
• 改进了 /model 的持久化设置，并能清晰区分项目锁定模型与用户选择模型
• 增强了 /resume 命令，在重新读取大型陈旧会话前会提供摘要选项
• 通过并发连接本地和 claude.ai MCP 服务器，显著提升了启动速度
• 升级了插件系统，支持自动解析并安装缺失的依赖项
• 在 macOS/Linux 上通过内置 bfs/ugrep 替代原有工具，提升搜索性能
• 将 Pro/Max 订阅者在支持模型上的默认推理力度（effort）提升至 high
• 修复了 OAuth 会话过期问题，现在支持在收到 401 错误时自动刷新令牌
• 修复了 Opus 4.7 的上下文窗口计算逻辑，防止过早触发自动压缩

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21117-Release-Notes-04-22)
[2026/4/22 21:01] Claude Code Changelog: Claude Code 2.1.118 Released

Key Updates:
• Added Vim visual mode (v) and visual-line mode (V) for enhanced text selection and manipulation
• Introduced custom theme support via /theme, allowing creation, editing, and plugin-shipped themes
• Merged /cost and /stats commands into a unified /usage interface
• Added DISABLE_UPDATES environment variable to strictly block all update paths
• Enhanced MCP tools with direct hook invocation and major OAuth/authentication stability fixes
• Improved Auto mode with a "Don't ask again" option and flexible $defaults rule configuration
• Fixed keyboard input freezes (Alt+K/X) and credential corruption issues on Linux/Windows

更新要点：
• 新增 Vim 可视模式 (v) 和行可视模式 (V)，支持文本选择与操作反馈
• 支持通过 /theme 创建和切换自定义主题，并允许插件分发主题文件
• 将 /cost 和 /stats 命令合并为统一的 /usage 界面
• 新增 DISABLE_UPDATES 环境变量，可完全禁用包括手动更新在内的所有更新路径
• 增强了 MCP 工具功能，支持通过 Hook 直接调用并修复了多项 OAuth 认证稳定性问题
• 优化了自动模式 (Auto mode)，增加“不再询问”选项并支持保留默认规则配置
• 修复了 Alt+K/X 导致的键盘输入冻结以及 Linux/Windows 平台下的凭据损坏问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21118-Release-Notes-04-23)
[2026/4/23 19:30] Claude Code Changelog: Claude Code 2.1.119 Released

Key Updates:
• Settings in /config (theme, editor mode, etc.) now persist to ~/.claude/settings.json with full override support
• Expanded --from-pr support to include GitLab merge requests, Bitbucket pull requests, and GitHub Enterprise
• Improved terminal experience by fixing extra blank lines on CRLF pastes and multi-line paste issues in Kitty protocol
• PowerShell tool commands can now be auto-approved in permission mode, matching existing Bash behavior
• Subagent and SDK MCP server reconfigurations now connect in parallel for faster performance
• Fixed fullscreen mode scrolling bug where the view would snap to the bottom whenever a tool finished
• Enhanced slash command UI with character highlighting and multi-line descriptions for better readability
• Fixed "Agent" tool isolation issues where stale worktrees were being reused across different sessions

更新要点：
• /config 设置（主题、编辑器模式等）现可持久化保存至配置文件，并支持项目级配置覆盖
• 扩展了 --from-pr 功能，现已支持 GitLab、Bitbucket 和 GitHub 企业版的拉取请求 URL
• 优化终端粘贴体验，修复了 Windows/Xcode 粘贴时产生多余空行及 Kitty 协议下的换行丢失问题
• PowerShell 工具命令现支持在权限模式下自动批准，与 Bash 行为保持一致
• 子代理（Subagent）与 SDK MCP 服务重连改为并行处理，显著提升连接速度
• 修复了全屏模式下的滚动问题，避免工具执行完成后视图自动跳转到底部
• 改进斜杠命令（/）界面，支持匹配字符高亮及长描述换行显示，提升易用性
• 修复了 Agent 工具在 worktree 隔离模式下会重用旧会话过时工作树的问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21119-Release-Notes-04-23)
[2026/4/24 20:31] Claude Code Changelog: Claude Code 2.1.120 Released

Key Updates:
• Windows users no longer require Git Bash; Claude Code now falls back to PowerShell automatically
• Added claude ultrareview subcommand for non-interactive code reviews in CI/CD pipelines
• Subprocesses now include AI_AGENT environment variable to help tools like gh attribute traffic
• Fixed terminal scrollback duplication and input issues when using --resume or interactive overlays
• Fixed a critical bug where the find tool could exhaust file descriptors and crash the host system
• Fixed telemetry settings not properly suppressing usage metrics for API and enterprise users
• VS Code: Voice dictation now respects user language settings and /usage opens the native dialog

更新要点：
• Windows 用户不再强制依赖 Git Bash，缺失时将自动切换至 PowerShell
• 新增 claude ultrareview 子命令，支持在 CI/CD 流水线中进行非交互式代码审查
• 子进程现包含 AI_AGENT 环境变量，便于 gh 等工具识别来自 Claude Code 的流量
• 修复了使用 --resume 或交互式界面时终端滚动回显重复及输入失效的问题
• 修复了 find 工具在大型目录树下耗尽文件描述符导致系统崩溃的关键错误
• 修复了遥测设置未能正确抑制 API 及企业用户的指标数据上传问题
• VS Code 插件：语音听写现支持语言设置，/usage 命令将打开原生账户面板

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21120-Release-Notes-04-25)
[2026/4/24 22:01] Claude Code Changelog: Claude Code 2.1.119 Released

Key Updates:
• Configuration settings (theme, editor mode, etc.) now persist to ~/.claude/settings.json
• Expanded --from-pr support to include GitLab, Bitbucket, and GitHub Enterprise URLs
• PowerShell tool commands can now be auto-approved, matching Bash behavior
• Subagent and SDK MCP server reconfiguration now connects servers in parallel for faster loading
• Fixed pasting issues where CRLF content or kitty keyboard protocol caused extra or missing newlines
• Fixed scrolling bug where fullscreen mode would snap to bottom after every tool execution
• Tool search is now disabled by default on Vertex AI to prevent unsupported header errors
• Improved slash command UI with better matching highlights and multi-line descriptions

更新要点：
• 配置设置（主题、编辑器模式等）现在将持久化保存至 ~/.claude/settings.json
• 扩展了 --from-pr 支持，现已兼容 GitLab、Bitbucket 和 GitHub Enterprise 链接
• PowerShell 工具命令现在支持自动批准，与 Bash 行为保持一致
• 子代理和 SDK MCP 服务重连改为并行处理，显著提升连接速度
• 修复了粘贴问题，解决了 CRLF 内容或 kitty 协议导致的空行或换行丢失故障
• 修复了全屏模式下每当工具执行完成页面就会自动回弹到底部的滚动问题
• 在 Vertex AI 上默认禁用工具搜索，以避免不支持的 Beta 请求头错误
• 优化了斜杠命令 UI，包括匹配字符高亮显示和长描述换行显示

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21119-Release-Notes-04-25)
[2026/4/27 21:00] Claude Code Changelog: Claude Code 2.1.121 Released

Key Updates:
• Added alwaysLoad config for MCP servers to skip tool-search deferral and auto-retry transient startup errors for better reliability
• Fixed critical memory leaks causing multi-GB RSS growth during image processing, /usage history, and long-running tool failures
• Resolved Bash tool crash when the initial working directory is deleted or moved mid-session, and fixed --resume crashes on corrupted transcripts
• Enhanced UX: added type-to-filter search for /skills, fixed fullscreen scroll jumping, and enabled scrollable terminal dialogs with arrow keys and mouse
• Expanded SDK/Dev capabilities: PostToolUse hooks now replace output for all tools, and CLAUDE_CODE_FORK_SUBAGENT=1 supports non-interactive sessions
• Improved configuration & cloud support: --dangerously-skip-permissions scope reduced, Vertex AI now supports X.509 certificate-based Workload Identity Federation

更新要点：
• 新增 MCP 服务器 alwaysLoad 配置以跳过工具搜索延迟，并自动重试启动时的临时错误以提升可靠性
• 修复了处理大量图片、/usage 历史记录及长运行工具失败时导致多 GB 内存增长的关键内存泄漏
• 解决了会话中途删除或移动初始工作目录导致 Bash 工具崩溃的问题，并修复了损坏转录导致的 --resume 启动崩溃
• 体验优化：为 /skills 新增打字过滤搜索，修复全屏模式下滚动跳底问题，并支持使用方向键和鼠标滚动终端对话框
• 扩展 SDK/开发者功能：PostToolUse 钩子现支持替换所有工具的输出，且 CLAUDE_CODE_FORK_SUBAGENT=1 支持非交互式会话
• 配置与云支持改进：缩小 --dangerously-skip-permissions 权限跳过范围，Vertex AI 现支持基于 X.509 证书的工作负载身份联合认证

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21121-Release-Notes-04-28-3)
[2026/4/28 18:31] Claude Code Changelog: Claude Code 2.1.122 Released

• Added ANTHROPIC_BEDROCK_SERVICE_TIER environment variable to select a Bedrock service tier (default, flex, or priority), sent as the X-Amzn-Bedrock-Service-Tier header
• Pasting a PR URL into the /resume search box now finds the session that created that PR (GitHub, GitHub Enterprise, GitLab, and Bitbucket)
• /mcp now shows claude.ai connectors hidden by a manually-added server with the same URL, with a hint to remove the duplicate
• Clarified the /mcp message shown when an MCP server is still unauthorized after the browser sign-in flow
• OpenTelemetry: numeric attributes on api_request/api_error log events are now emitted as numbers, not strings
• OpenTelemetry: added claude_code.at_mention log event for @-mention resolution
• Fixed /branch producing forks that fail with "tool_use ids were found without tool_result blocks" when the source session contained entries from rewound timelines
• Fixed /model not showing the Effort option for Bedrock application inference profile ARNs, and those ARNs not receiving output_config.effort
• Fixed Vertex AI / Bedrock returning invalid_request_error: output_config: Extra inputs are not permitted on session-title generation and other structured-output queries
• Fixed Vertex AI count_tokens endpoint returning 400 errors for users behind proxy gateways
• Fixed spinnerTipsOverride.excludeDefault not suppressing the time-based spinner tips
• Fixed ToolSearch missing MCP tools that connected after session start in nonblocking mode
• Fixed !exit / !quit in bash mode terminating the CLI instead of running as a shell command
• Fixed images sent to newer models being resized to 2576px per side instead of the correct 2000px maximum
• Fixed remote control session idle status redrawing twice per second, which could flood tmux -CC control pipes and pause the terminal
• Fixed assistant messages appearing blank in some sessions due to a stale view preference
• Fixed a malformed hooks entry in settings.json no longer invalidating the entire file
• Voice mode: keybindings bound to Caps Lock now show an error since terminals don't deliver Caps Lock as a key event

• 新增 ANTHROPIC_BEDROCK_SERVICE_TIER 环境变量，用于选择 Bedrock 服务层级（default、flex 或 priority），该值将作为 X-Amzn-Bedrock-Service-Tier 请求头发送
• 在 /resume 搜索框中粘贴 PR URL 现在可以定位到创建该 PR 的会话（支持 GitHub、GitHub Enterprise、GitLab 和 Bitbucket）
• /mcp 现在会显示被同 URL 手动添加的服务器所隐藏的 claude.ai 连接器，并提示移除重复项
• 明确了当 MCP 服务器在浏览器登录流程结束后仍未授权时显示的 /mcp 提示信息
• OpenTelemetry：api_request/api_error 日志事件中的数值属性现在以数字形式输出，而非字符串
• OpenTelemetry：新增 claude_code.at_mention 日志事件，用于处理 @ 提及解析
• 修复了当源会话包含来自回退时间线的条目时，/branch 生成的分支会因“tool_use ids were found without tool_result blocks”而失败的问题
• 修复了 /model 未为 Bedrock 应用推理配置 ARN 显示 Effort 选项，且这些 ARN 未接收 output_config.effort 的问题
• 修复了 Vertex AI / Bedrock 在生成会话标题及其他结构化输出查询时返回 invalid_request_error: output_config: Extra inputs are not permitted 的问题
• 修复了处于代理网关后的用户调用 Vertex AI count_tokens 端点时返回 400 错误的问题
• 修复了 spinnerTipsOverride.excludeDefault 未能屏蔽基于时间的加载提示的问题
• 修复了 ToolSearch 在非阻塞模式下遗漏了会话启动后连接的 MCP 工具的问题
• 修复了 bash 模式下执行 !exit / !quit 会直接终止 CLI 而非作为 Shell 命令运行
• 修复了发送给新模型的图片被错误缩放至每边 2576px，而非正确的最大 2000px 的问题
• 修复了远程控制会话空闲状态每秒重绘两次，可能导致 tmux -CC 控制管道过载并暂停终端的问题
• 修复了因过期的视图偏好设置导致部分会话中助手消息显示为空的问题
• 修复了 settings.json 中格式错误的 hooks 条目不再导致整个文件失效的问题
• 语音模式：绑定到 Caps Lock 的快捷键现在会显示错误提示，因为终端不会将 Caps Lock 作为按键事件传递
[2026/4/29 0:01] Claude Code Changelog: Claude Code 2.1.123 Released

• Fixed OAuth authentication failing with a 401 retry loop when CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 is set

• 修复了当设置 CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1 时 OAuth 认证失败并陷入 401 重试循环的问题
[2026/4/30 22:31] Claude Code Changelog: Claude Code 2.1.126 Released

Key Updates:
• Added claude project purge command to delete all project state and history
• /model picker now fetches available models from Anthropic-compatible gateways
• --dangerously-skip-permissions now bypasses prompts for protected paths like .claude/ and .git/
• OAuth login now supports manual code pasting for WSL2, SSH, and container environments
• Fixed session crashes caused by pasting images larger than 2000px (auto-downscaled on paste)
• Fixed "Stream idle timeout" errors during long model pauses or after Mac sleep
• Fixed garbled CJK text rendering on Windows and clipboard data exposure in process arguments
• Security fix: resolved allowManagedDomainsOnly and allowManagedReadPathsOnly settings being ignored

更新要点：
• 新增 claude project purge 命令以清除项目状态与历史记录
• /model 选择器现支持从 Anthropic 兼容网关获取可用模型
• --dangerously-skip-permissions 现可绕过 .claude/ 和 .git/ 等受保护路径的提示
• OAuth 登录现支持在 WSL2、SSH 和容器环境中手动粘贴验证码
• 修复粘贴超过 2000px 图片导致会话崩溃的问题（粘贴时自动缩放）
• 修复模型长时间思考或 Mac 休眠后出现的“流空闲超时”错误
• 修复 Windows 下中日韩文字乱码问题及剪贴板数据在进程参数中的泄露
• 安全修复：解决 allowManagedDomainsOnly 和 allowManagedReadPathsOnly 设置被忽略的问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21126-Release-Notes-05-01-2)
[2026/5/4 19:31] Claude Code Changelog: Claude Code 2.1.128 Released

Key Updates:
• --plugin-dir now accepts .zip archives for streamlined plugin distribution
• --channels supports console API key auth; managed orgs must enable channelsEnabled: true
• Breaking change: workspace is now a reserved MCP server name; existing servers will be skipped with a warning
• Subprocesses no longer inherit OTEL_* env vars, preventing CLI telemetry endpoint leakage
• Fixed crash loop when piping large stdin input (>10 MB) to claude -p
• Fixed EnterWorktree creating branches from local HEAD instead of origin, preventing unpushed commit loss

更新要点：
• --plugin-dir 现支持 .zip 归档文件，简化插件分发
• --channels 支持控制台 API 密钥认证；托管组织需启用 channelsEnabled: true
• 破坏性变更：workspace 现为保留的 MCP 服务器名；现有同名服务器将被跳过并提示警告
• 子进程不再继承 OTEL_* 环境变量，防止 CLI 遥测端点泄露
• 修复向 claude -p 管道输入大文件（>10 MB）时的崩溃循环问题
• 修复 EnterWorktree 从本地 HEAD 而非 origin 创建分支的问题，防止未推送的提交丢失

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21128-Release-Notes-05-04)
[2026/5/5 22:01] Claude Code Changelog: Claude Code 2.1.129 Released

Key Updates:
• Added --plugin-url flag to fetch plugin archives directly from a URL
• Introduced CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE for seamless background upgrades via Homebrew/WinGet
• Breaking change: Gateway model discovery is now opt-in via env var instead of automatic
• Fixed skillOverrides setting to properly control skill visibility and description collapsing
• Fixed critical OAuth refresh race condition causing unexpected logouts after system sleep
• Fixed prompt cache TTL silently downgrading from 1 hour to 5 minutes
• Fixed /context command wasting ~1.6k tokens by dumping ASCII grids into the chat

更新要点：
• 新增 --plugin-url 参数支持直接从 URL 获取插件压缩包
• 新增 CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE 环境变量，支持通过 Homebrew/WinGet 后台自动升级
• 破坏性变更：网关模型发现功能现改为通过环境变量手动开启，不再默认自动启用
• 修复 skillOverrides 设置，使其能正确控制技能可见性及描述折叠
• 修复 OAuth 刷新竞态条件导致的睡眠唤醒后意外登出问题
• 修复提示词缓存 TTL 被静默降级为 5 分钟的问题
• 修复 /context 命令错误输出 ASCII 网格导致浪费约 1.6k token 的问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21129-Release-Notes-05-06)
[2026/5/6 4:01] Claude Code Changelog: Claude Code 2.1.131 Released

• Fixed VS Code extension failing to activate on Windows due to a hardcoded build path in the bundled SDK (createRequire polyfill bug)
• Fixed Mantle endpoint authentication failing with missing x-api-key header

• 修复了因捆绑的 SDK 中存在硬编码构建路径（createRequire polyfill 错误）导致 VS Code 扩展在 Windows 上无法激活的问题
• 修复了因缺少 x-api-key 头导致 Mantle 端点身份验证失败的问题
[2026/5/6 18:31] Claude Code Changelog: Claude Code 2.1.132 Released

Key Updates:
• Added CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN env var to disable fullscreen renderer and use native scrollback
• Fixed critical unbounded memory growth (10GB+) when stdio MCP servers output non-protocol data
• Fixed graceful shutdown on external SIGINT to properly restore terminal modes instead of abrupt exit
• Fixed --resume session failures caused by emoji truncation and ignored --permission-mode flags
• Fixed terminal rendering issues including fullscreen blank screens, cursor misalignment, and broken pasting
• Improved MCP server status reporting with automatic retry on tool fetch failures and clearer auth states

更新要点：
• 新增 CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN 环境变量以禁用全屏渲染器并使用终端原生滚动
• 修复了 stdio MCP 服务器输出非协议数据时导致的严重内存无限增长（超 10GB）问题
• 修复了外部 SIGINT 信号下的优雅关闭逻辑，正确恢复终端模式而非直接崩溃退出
• 修复了 --resume 恢复会话时因 Emoji 截断导致的失败及 --permission-mode 标志被忽略的问题
• 修复了多项终端渲染问题，包括全屏黑屏、光标错位及粘贴功能异常
• 改进了 MCP 服务器状态报告，增加工具列表获取失败时的自动重试及更清晰的认证错误提示

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21132-Release-Notes-05-06)
[2026/5/7 20:01] Claude Code Changelog: Claude Code 2.1.133 Released

• Added worktree.baseRef setting (fresh | head) to choose whether --worktree, EnterWorktree, and agent-isolation worktrees branch from origin/<default> or local HEAD. Note: the default fresh changes EnterWorktree's base back to origin/<default> (it has been local HEAD since 2.1.128) — set worktree.baseRef: "head" to keep unpushed commits in new worktrees
• Added sandbox.bwrapPath and sandbox.socatPath managed settings (Linux/WSL) to specify custom bubblewrap and socat binary locations
• Added parentSettingsBehavior admin-tier key ('first-wins' | 'merge') to let admins opt SDK managedSettings (parent tier) into the policy merge
• Hooks now receive the active effort level via the effort.level JSON input field and the $CLAUDE_EFFORT environment variable, and Bash tool commands can read $CLAUDE_EFFORT
• Improved focus mode behavior
• Improved memory usage by releasing warm-spare background workers under memory pressure
• Fixed parallel sessions all dead-ending at 401 after a refresh-token race wiped shared credentials
• Fixed Edit/Write allow rules scoped to a drive root (C:\) or POSIX / matching incorrectly and always prompting
• Fixed an unhandled rejection (ECOMPROMISED) when a history or session-log file lock is compromised by clock skew or slow disk
• Fixed pressing Esc during conversation compaction showing a spurious "Error compacting conversation" notification
• Fixed HTTP(S)_PROXY / NO_PROXY / mTLS not being respected for the full MCP OAuth flow including discovery, dynamic client registration, token exchange, and token refresh
• Fixed Read/Write/Edit being denied on mapped network drives passed via --add-dir / SDK additionalDirectories
• Fixed Remote Control stop/interrupt from claude.ai not fully canceling the CLI session the same way local Esc does, causing queued messages to never advance after interrupting a stuck tool or prompt
• Fixed /effort in one session unexpectedly changing the effort level of other concurrent sessions, and a related issue where an IDE effort change could be silently dropped
• Fixed subagents not discovering project, user, or plugin skills via the Skill tool
• claude --help now lists --remote-control alongside --remote-control-session-name-prefix
• [VSCode] Fixed claudeCode.claudeProcessWrapper failing with "Unsupported platform" when the extension build doesn't bundle a Claude binary

• 新增 worktree.baseRef 设置（fresh | head），用于选择 --worktree、EnterWorktree 以及 Agent 隔离工作树是从 origin/<default> 还是本地 HEAD 分支。注意： 默认的 fresh 将 EnterWorktree 的基础分支改回 origin/<default>（自 2.1.128 起一直为本地 HEAD）—— 设置 worktree.baseRef: "head" 以在新工作树中保留未推送的提交
• 新增 sandbox.bwrapPath 和 sandbox.socatPath 托管设置（Linux/WSL），用于指定自定义 bubblewrap 和 socat 二进制文件路径
• 新增 parentSettingsBehavior 管理员层级键（'first-wins' | 'merge'），允许管理员选择将 SDK managedSettings（父层级）纳入策略合并
• Hook 现在通过 effort.level JSON 输入字段和 $CLAUDE_EFFORT 环境变量接收当前 effort 级别，且 Bash Tool 命令可读取 $CLAUDE_EFFORT
• 优化了专注模式行为
• 在内存压力较大时释放备用后台工作线程，从而优化内存使用
• 修复了在刷新令牌竞争导致共享凭证被清除后，并行会话全部在 401 状态卡死的问题
• 修复了作用域为驱动器根目录（C:\）或 POSIX / 的 Edit/Write 允许规则匹配不正确且始终弹出提示的问题
• 修复了在历史或会话日志文件锁因时钟偏差或磁盘缓慢而失效时，未处理的拒绝异常（ECOMPROMISED）
• 修复了在会话压缩期间按 Esc 键会显示误报的“压缩会话出错”通知的问题
• 修复了完整的 MCP OAuth 流程（包括发现、动态客户端注册、令牌交换和令牌刷新）未正确遵循 HTTP(S)_PROXY / NO_PROXY / mTLS 设置的问题
• 修复了通过 --add-dir / SDK additionalDirectories 传递的映射网络驱动器上 Read/Write/Edit 权限被拒绝的问题
• 修复了从 claude.ai 发起的远程控制停止/中断未能像本地按 Esc 键那样完全取消 CLI 会话，导致在中断卡住的工具或提示后，排队消息无法继续推进的问题
• 修复了在一个会话中使用 /effort 意外更改其他并发会话 effort 级别的问题，以及相关的 IDE effort 变更被静默丢弃的问题
• 修复了 Subagent 无法通过 Skill 工具发现项目、用户或插件 Skill 的问题
• claude --help 现在将 --remote-control 与 --remote-control-session-name-prefix 一同列出
• [VSCode] 修复了当扩展构建未捆绑 Claude 二进制文件时，claudeCode.claudeProcessWrapper 因“不支持的平台”而失败的问题
[2026/5/8 15:02] Claude Code Changelog: Claude Code 2.1.136 Released

Key Updates:
• Added settings.autoMode.hard_deny to unconditionally block specific inputs in auto mode
• Fixed MCP servers, plugins, and connectors disappearing after /clear in VS Code, JetBrains, and Agent SDK
• Fixed MCP OAuth refresh token loss during concurrent refreshes, eliminating daily re-authentication
• Fixed rare login loop caused by concurrent OAuth token overwrites
• Fixed @ file picker failing to locate mid-session created files or directories with over 100 entries
• WSL2 now supports image pasting from Windows clipboard via a PowerShell fallback
• Improved slash command dialog UX with standardized styling, immediate loading appearance, and fixed keyboard navigation
• Fixed environment variables from CLAUDE_ENV_FILE becoming stale after /resume or /clear

更新要点：
• 新增 settings.autoMode.hard_deny 设置，可在自动模式下无条件拦截特定输入
• 修复了 VS Code、JetBrains 和 Agent SDK 中 /clear 后 MCP 服务器、插件和连接器意外消失的问题
• 修复了并发刷新时 MCP OAuth 刷新令牌丢失的问题，不再需要每日重新认证
• 修复了因并发 OAuth 令牌覆盖导致的罕见登录循环问题
• 修复了 @ 文件选择器无法定位会话中创建的文件或超过 100 个条目的目录的问题
• WSL2 现支持通过 PowerShell 回退机制从 Windows 剪贴板粘贴图片
• 改进了斜杠命令对话框的交互体验，统一了样式、实现加载时即时显示并修复了键盘导航问题
• 修复了 /resume 或 /clear 后 CLAUDE_ENV_FILE 环境变量失效的问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21136-Release-Notes-05-08)
[2026/5/8 20:30] Claude Code Changelog: Claude Code 2.1.137 Released

• [VSCode] Fixed extension failing to activate on Windows

• [VSCode] 修复在 Windows 上扩展无法激活的问题
[2026/5/9 3:01] Claude Code Changelog: Claude Code 2.1.138 Released

• Internal fixes

• 内部修复
[2026/5/11 15:01] Claude Code Changelog: Claude Code 2.1.139 Released

Key Updates:
• Added Agent View (Research Preview) for centralized management of all Claude Code sessions
• Added /goal command to set completion conditions and enable multi-turn autonomous workflows
• Breaking change: Setting ANTHROPIC_API_KEY or related env vars now disables Claude.ai login features like Remote Control and /schedule
• Fixed critical deadlock that blocked authentication commands when credentials expired
• Fixed unbounded memory growth in MCP SSE servers by capping response bodies at 16 MB per frame
• Fixed bug where pasting or dropping multiple images only inserted the last one
• Added VSCode shortcut (Cmd/Ctrl+Shift+T) to reopen the most recently closed session tab

更新要点：
• 新增 Agent View（研究预览版），集中管理所有 Claude Code 会话
• 新增 /goal 命令，可设置完成条件并支持多轮自主工作流
• 破坏性变更：设置 ANTHROPIC_API_KEY 等环境变量将禁用 Claude.ai 登录功能（如 Remote Control 和 /schedule）
• 修复凭证过期时导致认证命令死锁的关键问题
• 修复 MCP SSE 服务器无限制内存增长问题，响应体已限制为每帧 16 MB
• 修复粘贴或拖拽多张图片时仅插入最后一张的缺陷
• 新增 VSCode 快捷键（Cmd/Ctrl+Shift+T）以重新打开最近关闭的会话标签页

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21139-Release-Notes-05-11-2)
[2026/5/12 17:31] Claude Code Changelog: Claude Code 2.1.140 Released

• Improved Agent tool subagent_type matching to accept case- and separator-insensitive values (e.g. "Code Reviewer" resolves to code-reviewer)
• Updated agent color palette
• Fixed /goal silently hanging when disableAllHooks or allowManagedHooksOnly is set — now shows a clear message instead of an indicator that never resolves
• Fixed a regression in settings hot-reload where symlinked settings files caused misattributed change events and spurious ConfigChange hooks
• Fixed claude --bg failing with "connection dropped mid-request" when the background service was about to idle-exit
• Fixed background service startup failing on machines with enterprise endpoint security by allowing more time
• Fixed remote managed settings not retrying on 401 — now retries once with a force-refreshed token
• Fixed managed extraKnownMarketplaces auto-update policy not being persisted to known_marketplaces.json
• Fixed /loop scheduling redundant wakeups to poll for background tasks that already notify on completion
• Fixed a recurring event-loop stall on Windows when a missing executable (e.g. gh) triggered synchronous where.exe re-spawns on every check
• Fixed Read tool calls failing validation when offset is passed as a whitespace-padded or +-prefixed string
• Fixed native terminal cursor not staying at the input caret when the terminal loses focus
• Plugins now warn when a default component folder (e.g. commands/) is silently ignored because plugin.json sets the matching key. Shown in /doctor, claude plugin list, and /plugin.

• 改进了 Agent 工具 subagent_type 的匹配逻辑，以接受不区分大小写和分隔符的值（例如 "Code Reviewer" 会解析为 code-reviewer）
• 更新了 Agent 配色方案
• 修复了当设置 disableAllHooks 或 allowManagedHooksOnly 时 /goal 静默挂起的问题 — 现在会显示明确的提示，而不是一个永远无法解析的指示器
• 修复了设置热重载中的回归问题，该问题导致符号链接的设置文件引发错误归因的变更事件和虚假的 ConfigChange 钩子
• 修复了当后台服务即将空闲退出时 claude --bg 报错“连接在请求中途断开”的问题
• 通过延长超时时间，修复了在启用企业端点安全策略的机器上后台服务启动失败的问题
• 修复了远程托管设置在收到 401 响应时未重试的问题 — 现在会使用强制刷新的 Token 重试一次
• 修复了托管的 extraKnownMarketplaces 自动更新策略未持久化到 known_marketplaces.json 的问题
• 修复了 /loop 调度了冗余的唤醒操作以轮询后台任务的问题，而这些任务在完成时本就会主动通知
• 修复了 Windows 上因缺少可执行文件（例如 gh）而在每次检查时触发同步 where.exe 重新生成，从而导致事件循环反复阻塞的问题
• 修复了当 offset 以填充空格的字符串或带 + 前缀的字符串形式传入时，Read 工具调用验证失败的问题
• 修复了终端失去焦点时原生终端光标未停留在输入光标处的问题
• 当默认组件文件夹（例如 commands/）因 plugin.json 设置了匹配键而被静默忽略时，插件现在会发出警告。该警告将在 /doctor、claude plugin list 和 /plugin 中显示。
[2026/5/13 19:31] Claude Code Changelog: Claude Code 2.1.141 Released

Key Updates:
• Added terminalSequence field to hooks for desktop notifications and bells without a controlling terminal
• Introduced CLAUDE_CODE_PLUGIN_PREFER_HTTPS and ANTHROPIC_WORKSPACE_ID env vars for flexible plugin cloning and workspace scoping
• Rewind menu now features "Summarize up to here" to compress historical context while preserving recent turns
• Fixed critical UX bugs: Ctrl+C now interrupts turns in Vim mode, and /model no longer alters autocompact thresholds in concurrent sessions
• Resolved security issue where desktop and third-party sessions incorrectly inherited host-managed API keys and auth tokens
• Improved background agent management: preserves permission modes, auto-retires idle workers after 5 minutes, and gracefully falls back on unhealthy workers
• Enhanced IDE/VSCode integration: restored "view diff in IDE" prompts, fixed microphone silence feedback, and improved plugin menu navigation

更新要点：
• 新增 terminalSequence 字段，使钩子无需控制终端即可触发桌面通知和提示音
• 新增 CLAUDE_CODE_PLUGIN_PREFER_HTTPS 和 ANTHROPIC_WORKSPACE_ID 环境变量，支持灵活克隆插件与限定工作区范围
• 重绕菜单新增“总结至此”功能，可在保留近期对话的同时压缩历史上下文
• 修复关键交互问题：Ctrl+C 现可正常中断 Vim 模式下的对话，且 /model 不再误改并发会话的自动压缩阈值
• 修复安全漏洞：桌面端与第三方提供商会话不再错误继承主机管理的 API 密钥与认证令牌
• 优化后台代理管理：保留权限模式，空闲代理 5 分钟后自动清理，并在预加热代理不健康时优雅降级
• 增强 IDE/VSCode 集成：恢复“在 IDE 中查看差异”提示，修复麦克风静音反馈，并优化插件菜单导航与搜索体验

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21141-Release-Notes-05-13)
[2026/5/14 19:01] Claude Code Changelog: Claude Code 2.1.142 Released

Key Updates:
• Added new claude agents flags to configure background sessions (e.g., --add-dir, --settings, --mcp-config)
• Fast mode now defaults to Opus 4.7, with an environment variable to pin to 4.6
• Fixed critical MCP_TOOL_TIMEOUT bug that ignored custom timeout values for remote servers
• Fixed daemon crash-looping after binary upgrades and macOS sleep/wake clock jumps
• Improved plugin discovery: root-level SKILL.md files are now automatically surfaced as skills
• Fixed background session instability with git worktrees, Windows network drives, and Chrome extension conflicts

更新要点：
• 新增 claude agents 配置参数以自定义后台会话（如 --add-dir、--settings、--mcp-config）
• 快速模式默认升级至 Opus 4.7，支持通过环境变量锁定为 4.6
• 修复了 MCP_TOOL_TIMEOUT 关键错误，该错误会导致远程服务器忽略自定义超时设置
• 修复了二进制升级及 macOS 睡眠唤醒后守护进程崩溃循环的问题
• 改进插件发现机制：根目录下的 SKILL.md 文件现自动识别为技能
• 修复了后台会话在 git worktree、Windows 网络驱动器及 Chrome 扩展冲突时的稳定性问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21142-Release-Notes-05-14)
[2026/5/15 18:31] Claude Code Changelog: Claude Code 2.1.143 Released

Key Updates:
• Added plugin dependency enforcement with copy-pasteable disable-chain hints and automatic transitive dependency enabling
• Introduced projected context cost (token estimates) in the /plugin marketplace for better usage tracking
• PowerShell tool enabled by default on Windows for Bedrock/Vertex/Foundry, with execution policy bypass and configurable opt-outs
• Enhanced background sessions: preserves model/effort after idle, honors default permissions, retains MCP/config across respawn, and fixes macOS file access errors
• Critical fixes: resolves CLI hangs from corrupt credentials, caps infinite stop-hook loops, and eliminates false-positive background stall detection
• claude agents dashboard now accepts --add-dir, --settings, --mcp-config, and permission/model flags for dispatched sessions
• Improved worktree safety: prevents accidental rm -rf fallback and adds worktree.bgIsolation: "none" for direct background editing

更新要点：
• 新增插件依赖强制检查，提供可复制的禁用链提示并自动启用传递依赖
• 插件市场浏览面板新增上下文成本（Token 估算）以优化使用追踪
• Windows 端默认启用 PowerShell 工具（支持 Bedrock/Vertex/Foundry），提供执行策略绕过与自定义关闭选项
• 后台会话体验增强：休眠恢复后保留模型/效率设置，默认权限生效，跨重启保留 MCP/配置，并修复 macOS 文件访问错误
• 关键修复：解决凭证损坏导致的 CLI 卡死、终止无限循环的停止钩子、消除后台假性停滞检测风暴
• claude agents 面板现支持 --add-dir、--settings、--mcp-config 及权限/模型参数以配置派生会话
• 提升 worktree 安全性：防止 rm -rf 误删回退，新增 worktree.bgIsolation: "none" 允许后台直接编辑工作副本

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21143-Release-Notes-05-15)
[2026/5/18 21:02] Claude Code Changelog: Claude Code 2.1.144 Released

Key Updates:
• Added /resume support for background sessions, allowing them to be managed alongside interactive ones
• Changed /model to apply only to the current session; press d to set a default for future sessions
• Fixed critical startup hang (up to 75s) when API is unreachable, now timing out after 15s
• Fixed MCP integration issues: paginated tool lists now fully load, and unsupported image MIME types no longer break conversations
• Fixed terminal display corruption on resize/long sessions and restored proper scrolling/input on Windows
• Improved SDK/headless MCP startup speed and added automatic recovery from rare stream stalls

更新要点：
• 新增 /resume 功能支持后台会话，可与交互式会话一同管理
• 变更 /model 行为：仅对当前会话生效，按 d 可为新会话设置默认模型
• 修复 API 不可达时启动卡死长达 75 秒的问题，现超时时间缩短至 15 秒
• 修复 MCP 集成问题：分页工具列表现已完整加载，不支持的图片 MIME 类型不再中断对话
• 修复终端在窗口调整或长会话中的显示错乱，并恢复 Windows 下的滚动与输入功能
• 提升 SDK/无头 MCP 启动速度，并新增罕见流中断的自动恢复机制

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21144-Release-Notes-05-19-2)
[2026/5/19 18:01] Claude Code Changelog: Claude Code 2.1.145 Released

• Added claude agents --json to list live Claude sessions as JSON for scripting (tmux-resurrect, status bars, session pickers)
• Added agent_id and parent_agent_id attributes to claude_code.tool OTEL spans, and fixed trace parenting so background subagent spans nest under the dispatching Agent tool span
• Status line JSON input now includes GitHub repo and PR information when detected
• /plugin Discover and Browse screens now show a plugin's commands, agents, skills, hooks, and MCP/LSP servers before installation
• claude agents terminal tab title now shows the awaiting-input count so an alt-tabbed window tells you when an agent needs attention
• Slash command and @-mention suggestion list now supports mouse hover and click in fullscreen mode
• Stop and SubagentStop hook input now includes background_tasks and session_crons fields
• Fixed a permission-prompt bypass where bare variable assignments to non-allowlisted environment variables in Bash commands were auto-approved
• Fixed MCP prompt slash commands showing raw server validation errors when a required argument is omitted — the error now names the missing argument and shows expected usage
• Fixed the spinner and elapsed-time display freezing until a keypress after the terminal was resized or refocused
• Fixed the cross-project resume hint failing in default Windows PowerShell 5.1 — Windows now uses ; as the command separator
• Fixed voice push-to-talk not working in the agent view's reply pane
• Fixed task lists rendering in random order when several tasks are created at once
• Fixed stale "Failed to install Anthropic marketplace" banner showing when the marketplace is already installed
• Fixed the PR badge in the footer not updating immediately after gh pr create and other PR-state-changing commands run in-session
• Fixed Agent Teams teammates with non-ASCII names failing every API call due to invalid header encoding
• Fixed /review using a deprecated projectCards GraphQL query that errored on repos with Classic Projects
• Fixed claude plugin validate not flagging skills: entries that point at a file instead of a directory — the error now suggests the parent directory
• Fixed an infinite loop where a skill using context: fork could repeatedly re-invoke itself instead of running
• Improved the Read tool to return a truncated first page with a "PARTIAL view" notice instead of a hard error when a whole-file read exceeds the token limit

• 新增 claude agents --json 以 JSON 格式列出活跃的 Claude 会话，便于脚本调用（如 tmux-resurrect、状态栏、会话选择器）
• 为 claude_code.tool OTEL spans 新增 agent_id 和 parent_agent_id 属性，并修复追踪父子关系，使后台 subagent spans 正确嵌套在分发 Agent 工具 span 之下
• 状态栏 JSON 输入现在在检测到时会包含 GitHub repo 和 PR 信息
• /plugin 的“发现”和“浏览”界面现在会在安装前显示插件的命令、agents、skills、hooks 以及 MCP/LSP 服务器
• claude agents 终端标签页标题现在会显示等待输入的计数，以便在 alt-tab 切换窗口时提示你何时需要 agent 介入
• Slash command 和 @-mention 建议列表现在在全屏模式下支持鼠标悬停和点击
• Stop 和 SubagentStop hook 输入现在包含 background_tasks 和 session_crons 字段
• 修复了 Permission 提示绕过问题：Bash 命令中对未列入白名单的环境变量进行直接变量赋值时会被自动批准
• 修复了 MCP prompt 斜杠命令在省略必填参数时显示原始服务器验证错误的问题——现在错误信息会指明缺失的参数并显示预期用法
• 修复了终端调整大小或重新获取焦点后，spinner 和耗时显示卡住直到按键才恢复的问题
• 修复了跨项目恢复提示在默认 Windows PowerShell 5.1 中失效的问题——Windows 现在使用 ; 作为命令分隔符
• 修复了 agent 视图回复窗格中语音 push-to-talk 功能无法使用的问题
• 修复了同时创建多个任务时，任务列表渲染顺序随机的问题
• 修复了当 marketplace 已安装时仍显示过时的“安装 Anthropic marketplace 失败”横幅的问题
• 修复了页脚中的 PR 徽章在 gh pr create 及其他更改 PR 状态的命令在会话内运行后未能立即更新的问题
• 修复了 Agent Teams 中名称包含非 ASCII 字符的队友因请求头编码无效导致每次 API 调用失败的问题
• 修复了 /review 使用已弃用的 projectCards GraphQL 查询，导致在包含 Classic Projects 的仓库中报错的问题
• 修复了 claude plugin validate 未标记 skills: 条目指向文件而非目录的问题——现在错误信息会建议改为父目录
• 修复了使用 context: fork 的 skill 陷入无限循环，反复自我调用而非正常运行的问题
• 改进了 Read 工具：当完整读取文件超出 token 限制时，不再直接报错，而是返回截断的第一页并附带“PARTIAL view”提示
[2026/5/20 22:01] Claude Code Changelog: Claude Code 2.1.146 Released

• Renamed /simplify to /code-review with an optional effort level (e.g. /code-review high)
• Auto mode no longer suppresses AskUserQuestion when the user or a skill explicitly relies on it
• Fixed Windows PowerShell tool failing with "command line is invalid" when pwsh is installed via winget or the Microsoft Store (regression in v2.1.124)
• Fixed MCP resources/list, resources/templates/list, and prompts/list dropping items past page 1 on paginating servers
• Fixed full-screen strobing in attached background sessions on Windows Terminal while Claude is streaming
• Fixed the auto-updater status line not showing your current version when an update fails
• Fixed on Windows, removing a background-job worktree no longer follows NTFS junctions into the main repo
• Fixed /background refusing sessions whose only typed input was a skill or custom slash command
• Fixed backgrounded sessions re-prompting for tool permissions you already granted with "don't ask again"
• Fixed /theme color editor and "New custom theme" dialogs not responding to Esc
• Fixed an uncaught exception at the end of streaming sessions when running via the Agent SDK
• Fixed forceLoginOrgUUID and forceLoginMethod managed-settings policies not being enforced against third-party-provider and API-key sessions
• Fixed GNOME Terminal right-click and middle-click paste not inserting text
• Fixed CLAUDE_CODE_SUBAGENT_MODEL not being forwarded to child processes in multi-agent sessions
• Improved auto-updater reliability: native version checks and downloads now retry transient network failures instead of failing immediately
• Improved diff rendering performance for large file edits

• 将 /simplify 重命名为 /code-review，并支持可选的投入级别（例如 /code-review high）
• 当用户或 Skill 明确依赖 AskUserQuestion 时，自动模式不再抑制该功能
• 修复了当通过 winget 或 Microsoft Store 安装 pwsh 时，Windows PowerShell 工具因“命令行无效”而失败的问题（v2.1.124 引入的回归问题）
• 修复了分页服务器上 MCP resources/list、resources/templates/list 和 prompts/list 在超过第 1 页时丢失项目的问题
• 修复了 Claude 进行 Streaming 时，Windows Terminal 中附加的 Background Task 会话出现全屏闪烁的问题
• 修复了更新失败时自动更新器状态栏未显示当前版本的问题
• 修复了 Windows 上移除 Background Task worktree 时，不再跟随 NTFS 联合点进入主仓库的问题
• 修复了 /background 拒绝仅包含 Skill 或自定义斜杠命令作为输入会话的问题
• 修复了 Background Task 会话对已勾选“不再询问”的工具权限重复请求授权的问题
• 修复了 /theme 颜色编辑器和“新建自定义主题”对话框无法响应 Esc 键的问题
• 修复了通过 Agent SDK 运行时，Streaming 会话结束时出现未捕获异常的问题
• 修复了 forceLoginOrgUUID 和 forceLoginMethod 管理设置策略未对第三方提供商和 API-key 会话生效的问题
• 修复了 GNOME Terminal 右键和中间键粘贴无法插入文本的问题
• 修复了多 Agent 会话中 CLAUDE_CODE_SUBAGENT_MODEL 未转发至子进程的问题
• 提升了自动更新器的可靠性：原生版本检查和下载现在会重试临时网络故障，而非立即失败
• 提升了大文件编辑时 diff 渲染的性能
[2026/5/21 20:03] Claude Code Changelog: Claude Code 2.1.147 Released

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21147-Release-Notes-05-22)
[2026/5/21 21:30] Claude Code Changelog: Claude Code 2.1.148 Released

• Fixed the Bash tool returning exit code 127 on every command for some users (a regression introduced in 2.1.147)

• 修复了部分用户执行每条命令时 Bash Tool 返回退出码 127 的问题（2.1.147 版本引入的回归问题）
[2026/5/22 18:31] Claude Code Changelog: Claude Code 2.1.149 Released

Key Updates:
• /usage now displays a per-category breakdown of limits usage, including skills, subagents, plugins, and MCP costs
• Enterprise: Added allowAllClaudeAiMcps setting to load claude.ai cloud MCP connectors alongside local managed configs
• Fixed critical PowerShell permission bypass where built-in cd commands could read files outside the workspace
• Fixed sandbox write allowlist in git worktrees to correctly restrict access to only the shared .git directory
• Fixed find command crashing the host on macOS when processing large directory trees
• Fixed /ultraplan and remote session creation failing when the working tree contains no uncommitted changes
• Improved /feedback reports to include conversation context prior to compaction for easier issue triage

更新要点：
• /usage 现显示按类别划分的限额使用情况，涵盖技能、子智能体、插件及 MCP 成本
• 企业版：新增 allowAllClaudeAiMcps 设置，支持在本地配置旁加载 claude.ai 云端 MCP 连接器
• 修复 PowerShell 关键权限绕过漏洞，内置 cd 命令曾可读取工作区外文件
• 修复 git worktree 中沙箱写入白名单限制，现仅允许访问共享 .git 目录
• 修复 find 命令在 macOS 上处理大型目录树时导致主机崩溃的问题
• 修复 /ultraplan 和远程会话创建在暂存区无变更时失败的问题
• 改进 /feedback 报告，包含上下文压缩前的对话记录，便于更快排查问题

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21149-Release-Notes-05-22)
[2026/5/23 0:30] Claude Code Changelog: Claude Code 2.1.150 Released

• Internal infrastructure improvements (no user-facing changes)

• 内部基础设施改进（无面向用户的变更）
[2026/5/26 22:01] Claude Code Changelog: Claude Code 2.1.152 Released

Key Updates:
• /code-review --fix and /simplify now apply review suggestions directly to your working tree
• Skills and slash commands can restrict model tools using disallowed-tools in frontmatter
• Added /reload-skills command and SessionStart hook support for dynamic skill reloading without restarts
• Claude Code now automatically switches to a fallback model instead of failing when the primary model is unavailable
• Fixed critical bugs: sessions stuck after model/login switches, remote MCP connection failures with egress proxies, and incorrect plugin deduplication

更新要点：
• /code-review --fix 和 /simplify 现在可直接将审查建议应用到工作目录
• 技能与斜杠命令可通过 frontmatter 中的 disallowed-tools 限制模型工具
• 新增 /reload-skills 命令与 SessionStart 钩子支持，无需重启即可动态重载技能
• 主模型不可用时，Claude Code 会自动切换至备用模型而非直接失败
• 修复关键问题：模型/登录切换后会话卡死、启用出口代理时远程 MCP 连接失败及插件去重错误

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21152-Release-Notes-05-27)
[2026/5/27 21:01] Claude Code Changelog: Claude Code 2.1.153 Released

Key Updates:
• /model now saves selection as default for new sessions; d keybinding replaced by s (requires keybindings.json update)
• macOS background agents now correctly register as "Claude Code" in Privacy & Security, preserving permissions across upgrades
• Fixed critical regressions: stateful MCP servers reconnect-looping, custom API gateway OAuth credential misrouting, and excessive memory usage during session resume
• claude agents autocomplete now suggests native slash commands and bundled skills for improved workflow discovery
• /bg command now properly continues active responses in background sessions instead of dropping them
• Fixed Windows PowerShell installer falsely reporting success on failure, with automatic rollback recovery for failed updates
• --strict-mcp-config now correctly preserves inline mcpServers in agent definitions and enforces subagent MCP allow/deny policies

更新要点：
• /model 现在将选择保存为新会话的默认模型；d 快捷键已替换为 s（需更新 keybindings.json）
• macOS 后台代理现在正确注册为“Claude Code”，跨版本升级时保留权限设置
• 修复关键回归问题：状态型 MCP 服务器重连循环、自定义 API 网关 OAuth 凭证路由错误，以及恢复会话时的内存溢出
• claude agents 自动补全现在推荐原生斜杠命令和内置技能，提升工作流发现效率
• /bg 命令现在能正确在后台会话中继续活跃响应，而非直接丢弃
• 修复 Windows PowerShell 安装器在失败时误报成功的问题，并新增更新失败后的自动回滚恢复功能
• --strict-mcp-config 现在正确保留代理定义中的内联 mcpServers，并严格执行子代理 MCP 允许/拒绝策略

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21153-Release-Notes-05-28)
[2026/5/28 14:31] Claude Code Changelog: Claude Code 2.1.154 Released

Key Updates:
• Opus 4.8 released with high effort as default, plus fast mode now offers 2.5x speed for 2x cost
• Dynamic workflows introduced: Claude can autonomously orchestrate complex tasks across multiple background agents
• Lean system prompt is now the default for all models except Haiku, Sonnet, and Opus 4.7 and earlier
• New ! <command> syntax enables running shell commands as detachable background sessions in claude agents
• Critical fixes: rm -rf $HOME now properly blocked, subagent worktree isolation secured, and auto-mode safety classifier token limit issue resolved
• Plugin system updated: plugins can now default to disabled and must be explicitly enabled via /plugin

更新要点：
• 发布 Opus 4.8 模型，默认启用高努力模式，快速模式现以 2 倍价格提供 2.5 倍速度
• 新增动态工作流功能：Claude 可自主编排多个后台代理以处理复杂任务
• 精简系统提示词现已成为除 Haiku、Sonnet 及 Opus 4.7 及更早版本外的默认设置
• claude agents 新增 ! <command> 语法，支持将 Shell 命令作为可分离的后台会话运行
• 关键修复：正确拦截 rm -rf $HOME 危险命令，修复子代理工作树隔离漏洞，并解决自动模式安全分类器误拦问题
• 插件系统更新：插件现可默认禁用，需通过 /plugin 命令显式启用

View Full Changelog | 查看完整更新日志 (https://telegra.ph/Claude-Code-21154-Release-Notes-05-28)
[2026/5/28 22:01] Claude Code Changelog: Claude Code 2.1.156 Released

• Fixed an issue when using Opus 4.8 where thinking blocks were modified, leading to API errors.

• 修复了在使用 Opus 4.8 时 Thinking Block 被修改，从而导致 API 错误的问题。